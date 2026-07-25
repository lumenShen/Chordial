<template>
  <div class="player-control-bar" ref="root">
    <!--
      玻璃扭曲层。
      - backdrop-filter 先对背后内容做磨砂/饱和
      - filter: url(#id) 再对模糊后的结果做边缘色散与折射
      - pointer-events:none 不拦截控件点击
    -->
    <span class="glass-warp" aria-hidden="true" :style="warpStyle" />

    <!-- 模糊封面背景 -->
    <div class="bar-bg" v-if="albumCoverUrl">
      <img :src="albumCoverUrl" alt="" />
    </div>

    <div class="controls-section">
      <!-- 专辑封面（真共享 DOM：动态 Teleport 到 PlayerView portal） -->
      <div class="album-cover-slot">
        <Teleport :to="coverPortalTarget" :disabled="!coverPortalTarget">
          <div class="album-cover-thumb" ref="coverThumb" v-if="albumCoverUrl" @click="openPlayerView" data-shared="cover">
            <img :src="albumCoverUrl" alt="专辑封面" />
          </div>
          <div class="album-cover-thumb placeholder" ref="coverThumb" v-else @click="openPlayerView" data-shared="cover">
            <i class="bi bi-disc-fill"></i>
          </div>
        </Teleport>
      </div>

      <!-- 歌曲信息（真共享 DOM：动态 Teleport 到 PlayerView portal） -->
      <div class="track-info-slot">
        <Teleport :to="metaPortalTarget" :disabled="!metaPortalTarget">
          <div class="track-info" v-if="trackTitle" data-shared="meta">
            <span class="track-title">{{ trackTitle }}</span>
            <span class="track-meta">{{ trackMeta }}</span>
          </div>
        </Teleport>
      </div>

      <button class="control-btn play-btn" @click="togglePlay">
        <i :class="isPlaying ? 'bi bi-pause-fill' : 'bi bi-play-fill'"></i>
      </button>

      <button class="control-btn" @click="$emit('next')">
        <i class="bi bi-skip-end-fill"></i>
      </button>
    </div>
  </div>

  <!--
    液态玻璃 SVG filter 定义（由 useLiquidGlass composable 提供参数）。
    - 完整 RGB 三通道色散实现
    - filter id 由 composable 的 useId 生成，多实例安全
  -->
  <svg class="liquid-glass-defs" aria-hidden="true" focusable="false">
    <defs>
      <radialGradient :id="`${lg.filterId}-edge-mask`" cx="50%" cy="50%" r="50%">
        <stop offset="0%" stop-color="black" stop-opacity="0" />
        <stop :offset="`${lg.edgeMaskMiddleOffset.value}%`" stop-color="black" stop-opacity="0" />
        <stop offset="100%" stop-color="white" stop-opacity="1" />
      </radialGradient>
      <filter :id="lg.filterId" x="-35%" y="-35%" width="170%" height="170%" color-interpolation-filters="sRGB">
        <feImage :href="lg.displacementMapUrl.value" :xlink:href="lg.displacementMapUrl.value" x="0" y="0" width="100%" height="100%" result="DISPLACEMENT_MAP" preserveAspectRatio="xMidYMid slice" />
        <feColorMatrix in="DISPLACEMENT_MAP" type="matrix" values="0.3 0.3 0.3 0 0 0.3 0.3 0.3 0 0 0.3 0.3 0.3 0 0 0 0 0 1 0" result="EDGE_INTENSITY" />
        <feComponentTransfer in="EDGE_INTENSITY" result="EDGE_MASK">
          <feFuncA type="discrete" :tableValues="lg.edgeMaskTableValues.value" />
        </feComponentTransfer>
        <feOffset in="SourceGraphic" dx="0" dy="0" result="CENTER_ORIGINAL" />
        <feDisplacementMap in="SourceGraphic" in2="DISPLACEMENT_MAP" :scale="lg.redScale.value" xChannelSelector="R" yChannelSelector="B" result="RED_DISPLACED" />
        <feColorMatrix in="RED_DISPLACED" type="matrix" values="1 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 1 0" result="RED_CHANNEL" />
        <feDisplacementMap in="SourceGraphic" in2="DISPLACEMENT_MAP" :scale="lg.greenScale.value" xChannelSelector="R" yChannelSelector="B" result="GREEN_DISPLACED" />
        <feColorMatrix in="GREEN_DISPLACED" type="matrix" values="0 0 0 0 0 0 1 0 0 0 0 0 0 0 0 0 0 0 1 0" result="GREEN_CHANNEL" />
        <feDisplacementMap in="SourceGraphic" in2="DISPLACEMENT_MAP" :scale="lg.blueScale.value" xChannelSelector="R" yChannelSelector="B" result="BLUE_DISPLACED" />
        <feColorMatrix in="BLUE_DISPLACED" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 1 0 0 0 0 0 1 0" result="BLUE_CHANNEL" />
        <feBlend in="GREEN_CHANNEL" in2="BLUE_CHANNEL" mode="screen" result="GB_COMBINED" />
        <feBlend in="RED_CHANNEL" in2="GB_COMBINED" mode="screen" result="RGB_COMBINED" />
        <feGaussianBlur in="RGB_COMBINED" :stdDeviation="lg.aberrationBlur.value" result="ABERRATED_BLURRED" />
        <feComposite in="ABERRATED_BLURRED" in2="EDGE_MASK" operator="in" result="EDGE_ABERRATION" />
        <feComponentTransfer in="EDGE_MASK" result="INVERTED_MASK">
          <feFuncA type="table" tableValues="1 0" />
        </feComponentTransfer>
        <feComposite in="CENTER_ORIGINAL" in2="INVERTED_MASK" operator="in" result="CENTER_CLEAN" />
        <feComposite in="EDGE_ABERRATION" in2="CENTER_CLEAN" operator="over" />
      </filter>
    </defs>
  </svg>
</template>

<script setup>
/**
 * PlayerControlBar - 播放器控制栏
 *
 * 提供播放控制、歌曲信息展示、封面模糊背景等功能。
 * 从 PlayerStore 读取当前播放状态和歌曲元数据。
 */
import { computed, onMounted, useTemplateRef } from 'vue'
import { PlayerStore } from '@/stores/player.js'
import { usePerf } from '@/utils/performanceMonitor.js'
import { useAnime } from '@/composables/useAnime.js'
import { useLiquidGlass } from '@/composables/useLiquidGlass.js'

const { log } = usePerf('PlayerControlBar')

const rootRef = useTemplateRef('root')
const coverThumbRef = useTemplateRef('coverThumb')
const { run } = useAnime(() => rootRef.value)

// ===== 液态玻璃 composable =====
// 液态玻璃 composable（使用全局默认参数）
const lg = useLiquidGlass()

// warp 层样式：backdrop-filter 包含 blur + saturate + url(#id)
// url(#id) 必须在 backdrop-filter 里，让 SVG 滤镜作用于 backdrop（玻璃背后的内容）
const warpStyle = computed(() => ({
  backdropFilter: lg.backdropFilter.value,
  WebkitBackdropFilter: lg.backdropFilter.value,
}))

// 真共享 DOM：动态 Teleport 目标（PlayerView 打开时设为 portal 容器）
const coverPortalTarget = computed(() => PlayerStore.coverPortalTarget.value)
const metaPortalTarget = computed(() => PlayerStore.metaPortalTarget.value)

// 入场动画：控制栏向上淡入
onMounted(() => {
  run(({ animate, presets }) => {
    animate(rootRef.value, { ...presets.fadeInUp })
  })
  // 绑定 glassRef 到控制栏根元素
  lg.glassRef.value = rootRef.value
})

const props = defineProps({
  currentTime: {
    type: Number,
    default: 0
  },
  duration: {
    type: Number,
    default: 0
  },
  isPlaying: {
    type: Boolean,
    default: false
  },
  volume: {
    type: Number,
    default: 0.8
  },
  playMode: {
    type: String,
    default: 'sequence'
  },
  muted: {
    type: Boolean,
    default: false
  },
  albumCoverUrl: {
    type: String,
    default: null
  }
})

const emit = defineEmits([
  'play',
  'pause',
  'seek',
  'volume-change',
  'toggle-play-mode',
  'previous',
  'next',
  'toggle-mute'
])

const trackTitle = computed(() => PlayerStore.state.currentTrack?.title ?? '')
const trackArtist = computed(() => PlayerStore.state.currentTrack?.artist ?? '')
const trackAlbum  = computed(() => PlayerStore.state.currentTrack?.albumTitle ?? '')

const trackMeta = computed(() => {
  const parts = [trackArtist.value, trackAlbum.value].filter(Boolean)
  return parts.join(' · ')
})

// 播放模式图标
const playModeIcon = computed(() => {
  switch (props.playMode) {
    case 'random':
      return 'bi bi-shuffle'
    case 'loop':
      return 'bi bi-repeat'
    case 'loop_one':
      return 'bi bi-repeat-1'
    default:
      return 'bi bi-arrow-right'
  }
})

// 音量图标
const volumeIcon = computed(() => {
  if (props.muted || props.volume === 0) return 'bi bi-volume-mute-fill'
  if (props.volume < 0.3) return 'bi bi-volume-off-fill'
  if (props.volume < 0.7) return 'bi bi-volume-down-fill'
  return 'bi bi-volume-up-fill'
})

// 点击封面打开 PlayerView 模态（传递封面 DOM 元素用于 FLIP 共享元素动画）
const openPlayerView = () => {
  PlayerStore.openPlayerView(coverThumbRef.value)
}

// 切换播放/暂停
const togglePlay = () => {
  if (props.isPlaying) {
    log('togglePlay', { action: 'pause' });
    emit('pause')
  } else {
    log('togglePlay', { action: 'play' });
    emit('play')
  }
}

// 跳转
const onSeek = (e) => {
  emit('seek', parseFloat(e.target.value))
}

// 音量变化
const onVolumeChange = (e) => {
  emit('volume-change', parseFloat(e.target.value))
}

// 格式化时间
const formatTime = (seconds) => {
  if (!seconds || isNaN(seconds)) return '0:00'
  const mins = Math.floor(seconds / 60)
  const secs = Math.floor(seconds % 60)
  return `${mins}:${secs.toString().padStart(2, '0')}`
}
</script>

<style scoped>
.player-control-bar {
  display: flex;
  flex-direction: column;
  position: fixed;
  bottom: calc(1.5rem + var(--safe-area-bottom));
  right: 0.9rem;
  height: var(--bottom-nav-height);
  padding: 0.5rem 1rem;
  transition: bottom var(--transition-slow);
  border-radius: calc(var(--bottom-nav-height) * 0.6);
  background: var(--bg-glass);
  border: 1px solid var(--border-light);
  justify-content: center;
  overflow: hidden;
  isolation: isolate;
  /* GPU: 悬浮控制条合成层 */
  contain: layout paint;
}

/* 玻璃扭曲层：backdrop-filter 做磨砂，filter 做边缘色散 */
.glass-warp {
  position: absolute;
  inset: 0;
  border-radius: inherit;
  pointer-events: none;
  z-index: 0;
  will-change: backdrop-filter, filter;
}

/* SVG defs：仅承载 filter 定义，不占布局空间 */
.liquid-glass-defs {
  position: absolute;
  width: 0;
  height: 0;
  pointer-events: none;
  overflow: hidden;
}

/* 模糊封面背景层 */
.bar-bg {
  position: absolute;
  inset: 0;
  z-index: -1;
  border-radius: inherit;
  overflow: hidden;
}

.bar-bg img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  filter: blur(18px) brightness(0.55) saturate(1.3);
  transform: scale(1.15);
  /* GPU: 封面模糊层固定合成，限制重绘 */
  will-change: filter;
  contain: paint;
}

.bar-bg::after {
  content: '';
  position: absolute;
  inset: 0;
  background: linear-gradient(
    160deg,
    rgba(0, 0, 0, 0.35) 0%,
    rgba(0, 0, 0, 0.05) 45%,
    rgba(0, 0, 0, 0.2) 100%
  );
}

.controls-section {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  position: relative;
  z-index: 1;
}

/* 真共享 DOM：teleport 后占位容器保持原尺寸，避免布局塌陷 */
.album-cover-slot {
  width: 2.5rem;
  height: 2.5rem;
  flex-shrink: 0;
}
.track-info-slot {
  min-width: 0;
  max-width: 12rem;
  flex: 1;
}

.control-btn {
  background: none;
  border: none;
  font-size: 1.25rem;
  cursor: pointer;
  padding: 0.2rem 0.5rem 0 0.5rem;
  opacity: 0.85;
  transition: opacity 0.2s;
  flex-shrink: 0;
  color: var(--text-primary);
}

/* 有封面背景时切换为白色文字 */
.bar-bg ~ .controls-section .control-btn {
  color: rgba(255, 255, 255, 0.92);
}

.control-btn:hover {
  opacity: 1;
}

.play-btn {
  font-size: 1.75rem;
}

/* 专辑封面缩略图 */
.album-cover-thumb {
  width: 2.5rem;
  border: 1px solid var(--border-light);
  height: 2.5rem;
  border-radius: 0.5rem;
  overflow: hidden;
  flex-shrink: 0;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.25);
  cursor: pointer;
  transition: transform 0.2s ease;
}

.album-cover-thumb:hover {
  transform: scale(1.08);
}

.album-cover-thumb:active {
  transform: scale(0.95);
}

.album-cover-thumb img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
}

.album-cover-thumb.placeholder {
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--bg-glass);
  border: 1px solid var(--border-light);
  box-shadow: none;
}

.album-cover-thumb.placeholder i {
  font-size: 1.25rem;
  opacity: 0.5;
}

/* 歌曲信息 */
.track-info {
  display: flex;
  flex-direction: column;
  min-width: 0;
  max-width: 12rem;
  line-height: 1.25;
}

.track-title {
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.bar-bg ~ .controls-section .track-title {
  color: rgba(255, 255, 255, 0.95);
}

.track-meta {
  font-size: 0.6875rem;
  color: var(--text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  margin-top: 1px;
}

.bar-bg ~ .controls-section .track-meta {
  color: rgba(255, 255, 255, 0.6);
}

@media (max-width: 767px) {
  .player-control-bar {
    right: 1rem;
    left: 1rem;
    /* bottom: calc(var(--bottom-nav-height) + 0.5rem); */
    /* border-radius: 1rem; */
    /* padding: 0.4rem 0.75rem; */
  }

  .controls-section {
    gap: 0.5rem;
  }

  .track-info {
    flex: 1;
    max-width: none;
  }
}
</style>
