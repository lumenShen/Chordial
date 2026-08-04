/**
 * useChordPlayer — 自研 Vue 播放器（Chordial Player）的状态桥接层。
 *
 * 将 PlayerStore（全局播放状态）与 AmllSettingsStore（播放器配置）桥接到
 * 自研播放器所需的响应式状态。复用 PlayerStore 的权威状态与 actions，
 * 保证与 AMLL 引擎共用同一套播放内核，切引擎无状态冲突。
 *
 * # 设计要点
 * - 歌词用 per-frame 高精度时间驱动逐字高亮与歌词滚动（对齐 AMLL 的 setCurrentTime）
 * - currentTime 由 rAF 读取 audio.currentTime，比 PlayerStore 的 100ms 节流更平滑
 * - 所有 actions 直接委托 PlayerStore，封装统一错误处理
 * - 曲目/歌词变化经 Vue watch 自动同步封面与歌词行
 */
import { computed, ref, readonly, watch, provide, onScopeDispose } from 'vue'
import { PlayerStore, PlayMode } from '@/stores/player/index.js'
import { AmllSettingsStore } from '@/stores/amllSettings.js'
import { parseLyrics } from '@/utils/lyricConverter.js'
import { CHORD_NOW_MS } from '@/views/player/composables/injectionKeys.js'

// ── 播放模式元信息（用于 UI 图标/文案）─────────────────────────
export const PLAY_MODE_META = {
  [PlayMode.SEQUENCE]: { label: '顺序播放', icon: 'repeat' },
  [PlayMode.LOOP]:     { label: '列表循环', icon: 'repeat' },
  [PlayMode.LOOP_ONE]: { label: '单曲循环', icon: 'repeat-once' },
  [PlayMode.RANDOM]:   { label: '随机播放', icon: 'shuffle' },
}

/** 循环按钮：SEQUENCE → LOOP → LOOP_ONE → SEQUENCE（RANDOM 下切到 LOOP） */
function cyclePlayMode(current) {
  switch (current) {
    case PlayMode.SEQUENCE: return PlayMode.LOOP
    case PlayMode.LOOP:     return PlayMode.LOOP_ONE
    case PlayMode.LOOP_ONE: return PlayMode.SEQUENCE
    case PlayMode.RANDOM:   return PlayMode.LOOP
    default:                return PlayMode.LOOP
  }
}

/** 随机按钮：RANDOM → SEQUENCE；否则 → RANDOM */
function toggleShuffle(current) {
  return current === PlayMode.RANDOM ? PlayMode.SEQUENCE : PlayMode.RANDOM
}

/**
 * 将歌词字符串解析为带逐字信息的行（对齐 AMLL LyricLine 结构）。
 * @param {string} str
 * @returns {Array<{startTime:number,endTime:number,words:Array,translatedLyric:string,romanLyric:string}>}
 */
function parseChordLyrics(str) {
  if (!str || typeof str !== 'string') return []
  try {
    const parsed = parseLyrics(str)
    if (!parsed || parsed.length === 0) return []
    parsed.sort((a, b) => (a.time ?? 0) - (b.time ?? 0))
    return parsed.map((line, i) => {
      const startTime = line.time ?? 0
      const next = parsed[i + 1]
      const nextStart = next ? (next.time ?? 0) : undefined
      const endTime = line.endTime
        ?? (nextStart > startTime ? nextStart : startTime + (line.duration || 5000))
      const words = (line.words && line.words.length > 0)
        ? line.words.map((w) => ({
            word: w.text || '',
            startTime: w.time ?? startTime,
            endTime: w.time + (w.duration || 200),
          }))
        : [{ word: line.text || '', startTime, endTime }]
      return {
        startTime,
        endTime,
        words,
        translatedLyric: line.translation || '',
        romanLyric: '',
      }
    })
  } catch (err) {
    console.warn('[useChordPlayer] 歌词解析失败:', err)
    return []
  }
}

/**
 * 创建自研播放器状态桥接。
 * @returns {object} 播放器状态与 actions
 */
export function useChordPlayer() {
  const state = readonly(PlayerStore.state)
  const settings = AmllSettingsStore.state

  // ── 响应式状态 ─────────────────────────────────────────────
  // currentTime 用 rAF 高精度驱动（毫秒）
  const currentTimeMs = ref(0)
  // 当前活跃歌词行索引（-1 表示无歌词/无匹配）
  const activeLineIndex = ref(-1)
  // 已缓冲时长（毫秒），来自 audio.buffered（低频率更新）
  const bufferMs = ref(0)
  // 歌词行（解析后的）
  const lyricLines = ref([])
  // 封面 URL
  const coverUrl = ref('')
  // 沉浸模式（隐藏控件）
  const isImmersive = ref(false)

  // ── 派生计算属性 ───────────────────────────────────────────
  const track = computed(() => state.currentTrack)
  const isPlaying = computed(() => state.isPlaying)
  const isLoading = computed(() => state.isLoading)
  const durationMs = computed(() => (state.duration || 0) * 1000)
  const volume = computed(() => state.volume)
  const muted = computed(() => state.muted)
  const playMode = computed(() => state.playMode)
  const isShuffle = computed(() => state.playMode === PlayMode.RANDOM)
  const isRepeatAll = computed(() => state.playMode === PlayMode.LOOP)
  const isRepeatOne = computed(() => state.playMode === PlayMode.LOOP_ONE)

  const title = computed(() => track.value?.title || '未知歌曲')
  const artists = computed(() => (track.value?.artistNames || []).join(' / ') || '未知歌手')
  // 歌手列表（含 id，供点击跳转，对齐 AMLL onArtistClicked）
  const artistList = computed(() => (track.value?.artistNames || []).map((name, i) => ({
    name: name || '未知歌手',
    id: track.value?.artistIds?.[i] || name || '',
  })))
  const album = computed(() => track.value?.albumTitle || '')
  const hasLyrics = computed(() => lyricLines.value.length > 0)

  // 进度百分比 0-100
  const progressPercent = computed(() => {
    if (!durationMs.value) return 0
    return Math.min(100, (currentTimeMs.value / durationMs.value) * 100)
  })

  // 格式化的当前时间 / 总时长 / 剩余时间
  const formatTime = (ms) => {
    if (!Number.isFinite(ms)) ms = 0
    const isRemain = ms < 0
    const total = Math.abs(Math.floor(ms / 1000))
    const m = Math.floor(total / 60)
    const s = total % 60
    return `${isRemain ? '-' : ''}${m}:${String(s).padStart(2, '0')}`
  }
  const formattedCurrentTime = computed(() => formatTime(currentTimeMs.value))
  const formattedDuration = computed(() => formatTime(durationMs.value))
  const formattedRemaining = computed(() => formatTime(currentTimeMs.value - durationMs.value))

  // ── 封面加载（带取消令牌，防快速切歌旧封面覆盖新封面）────────
  let coverToken = 0
  async function loadCover() {
    const myToken = ++coverToken
    const t = track.value
    if (!t) { coverUrl.value = ''; return }
    try {
      const { url } = await t.acquireCoverResource('large')
      if (myToken !== coverToken) return
      coverUrl.value = url
    } catch {
      if (myToken !== coverToken) return
      coverUrl.value = ''
    }
  }

  // ── 歌词加载（同步解析当前 lyricsData）──────────────────────
  function loadLyrics() {
    const src = state.lyricsData?.syncedLyrics || state.lyricsData?.plainLyrics || ''
    lyricLines.value = parseChordLyrics(src)
    activeLineIndex.value = -1
  }

  // ── rAF 高精度时间循环（驱动歌词高亮 + 进度）────────────────
  // 仅当 currentTime 实际发生变化时才覆写 ref，避免每帧无谓触发订阅者。
  // 停顿/切歌时 currentTime 稳定，不会产生重渲染。
  let rafId = 0
  let lastTimeMs = -1
  let lastBufferMs = -1
  function tick() {
    const audio = PlayerStore.getAudioElement()
    if (audio && Number.isFinite(audio.currentTime)) {
      const t = audio.currentTime * 1000
      if (Math.abs(t - lastTimeMs) > 0.5) {
        lastTimeMs = t
        currentTimeMs.value = t
      }
      // 已缓冲时长（audio.buffered 末尾区间），仅在变化时更新
      try {
        const b = audio.buffered
        if (b && b.length > 0) {
          const end = b.end(b.length - 1) * 1000
          if (Math.abs(end - lastBufferMs) > 100) {
            lastBufferMs = end
            bufferMs.value = end
          }
        }
      } catch { /* 忽略非安全上下文的异常 */ }
    }
    // 计算活跃歌词行（二分查找）
    const lines = lyricLines.value
    if (lines.length > 0) {
      const t = currentTimeMs.value
      let lo = 0, hi = lines.length - 1, idx = -1
      while (lo <= hi) {
        const mid = (lo + hi) >> 1
        if (lines[mid].startTime <= t) { idx = mid; lo = mid + 1 }
        else hi = mid - 1
      }
      if (idx !== activeLineIndex.value) activeLineIndex.value = idx
    }
    rafId = requestAnimationFrame(tick)
  }

  function startTick() {
    if (!rafId) rafId = requestAnimationFrame(tick)
  }
  function stopTick() {
    if (rafId) { cancelAnimationFrame(rafId); rafId = 0 }
  }

  // 暂停时停止 rAF（低配设备省电）；恢复时继续
  const stopTickPauseWatch = watch(
    () => PlayerStore.state.isPlaying,
    (playing) => { if (playing) startTick(); else stopTick() },
    { immediate: true },
  )

  // ── 状态订阅（PlayerStore 变化 → 同步自研状态）──────────────
  const stopTrackWatch = watch(
    () => PlayerStore.state.currentTrack,
    () => { loadCover(); loadLyrics() },
    { immediate: true },
  )
  const stopLyricsWatch = watch(
    () => PlayerStore.state.lyricsData?.syncedLyrics || PlayerStore.state.lyricsData?.plainLyrics || '',
    () => loadLyrics(),
  )

  // ── actions（委托 PlayerStore）─────────────────────────────
  const togglePlay = () => PlayerStore.togglePlay()
  const playPrevious = () => PlayerStore.playPrevious()
  const playNext = () => PlayerStore.playNext()
  const seek = (ms) => PlayerStore.seek(ms / 1000)
  const seekToPercent = (p) => PlayerStore.seekToPercent(p)
  const setVolume = (v) => PlayerStore.setVolume(v)
  const toggleMute = () => PlayerStore.toggleMute()
  const cycleRepeat = () => PlayerStore.setPlayMode(cyclePlayMode(state.playMode))
  const toggleShuffleMode = () => PlayerStore.setPlayMode(toggleShuffle(state.playMode))
  const closePlayerView = () => PlayerStore.closePlayerView()
  const openTrackMenu = () => PlayerStore.openTrackMenu()
  const openPlayQueue = () => PlayerStore.openPlayQueue()
  const registerInteraction = () => PlayerStore.registerInteraction()

  // 歌词行点击 → seek
  const onLyricLineClick = (line) => {
    if (line && Number.isFinite(line.startTime)) seek(line.startTime)
  }

  // 高频播放进度通过 provide 下发，仅进度条与活动歌词行订阅，
  // 避免经 props 下发导致整棵播放器树每帧重渲染。
  provide(CHORD_NOW_MS, currentTimeMs)

  // 清理
  onScopeDispose(() => {
    stopTick()
    stopTrackWatch()
    stopLyricsWatch()
    stopTickPauseWatch()
  })

  return {
    state,
    settings,
    currentTimeMs,
    bufferMs,
    activeLineIndex,
    lyricLines,
    coverUrl,
    isImmersive,
    track,
    isPlaying,
    isLoading,
    durationMs,
    volume,
    muted,
    playMode,
    isShuffle,
    isRepeatAll,
    isRepeatOne,
    title,
    artists,
    artistList,
    album,
    hasLyrics,
    progressPercent,
    formattedCurrentTime,
    formattedDuration,
    formattedRemaining,
    togglePlay,
    playPrevious,
    playNext,
    seek,
    seekToPercent,
    setVolume,
    toggleMute,
    cycleRepeat,
    toggleShuffleMode,
    closePlayerView,
    openTrackMenu,
    openPlayQueue,
    registerInteraction,
    onLyricLineClick,
    formatTime,
  }
}

export default useChordPlayer