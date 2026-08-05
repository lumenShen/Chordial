<template>
  <div ref="rootEl" class="chord-player-view" :class="{ 'is-immersive': isImmersive }">
    <!-- 流体背景层（铺满） -->
    <div class="chord-background">
      <ChordFluidBackground
        :cover-url="coverUrl"
        :static-mode="settings.lyricBackgroundStaticMode"
        :fps="settings.lyricBackgroundFPS"
        :vsync="settings.lyricBackgroundVSync"
        :render-scale="settings.lyricBackgroundRenderScale"
      />
    </div>

    <!-- 自动布局：横屏 / 竖屏 -->
    <ChordHorizontalLayout
      v-if="!isVertical"
      :title="title"
      :artists="artists"
      :artist-list="artistList"
      :album="album"
      :quality-label="qualityLabel"
      :cover-url="coverUrl"
      :show-music-name="settings.showMusicName"
      :show-music-artists="settings.showMusicArtists"
      :show-music-album="settings.showMusicAlbum"
      :lyric-lines="lyricLines"
      :active-line-index="activeLineIndex"
      :show-translation="settings.enableLyricTranslationLine"
      :font-size-px="fontSizePx"
      :font-weight="settings.lyricFontWeight"
      :letter-spacing="settings.lyricLetterSpacing"
      :font-family="settings.lyricFontFamily"
      :enable-blur="settings.enableLyricLineBlurEffect"
      :enable-scale="settings.enableLyricLineScaleEffect"
      :enable-spring="settings.enableLyricLineSpringAnimation"
      :duration-ms="durationMs"
      :buffer-ms="bufferMs"
      :formatted-current-time="formattedCurrentTime"
      :formatted-duration="formattedDuration"
      :formatted-remaining="formattedRemaining"
      :is-playing="isPlaying"
      :is-loading="isLoading"
      :is-shuffle="isShuffle"
      :is-repeat-all="isRepeatAll"
      :is-repeat-one="isRepeatOne"
      :volume="volume"
      :muted="muted"
      :show-volume-control="settings.showVolumeControl"
      :show-remaining-time="settings.showRemainingTime"
      :show-bottom-controls="showBottomControls"
      :is-immersive="isImmersive"
      :hide-lyric="hideLyric"
      @close="closePlayerView"
      @open-menu="openTrackMenu"
      @seek="seek"
      @toggle-play="togglePlay"
      @prev="playPrevious"
      @next="playNext"
      @toggle-shuffle="toggleShuffleMode"
      @cycle-repeat="cycleRepeat"
      @toggle-mute="toggleMute"
      @volume="setVolume"
      @toggle-remaining="onToggleRemaining"
      @line-click="onLyricLineClick"
      @background-click="toggleImmersive"
      @open-queue="openPlayQueue"
      @toggle-lyric="toggleHideLyric"
      @airplay="onAirplay"
    />

    <ChordVerticalLayout
      v-else
      :title="title"
      :artists="artists"
      :artist-list="artistList"
      :album="album"
      :quality-label="qualityLabel"
      :cover-url="coverUrl"
      :show-music-name="settings.showMusicName"
      :show-music-artists="settings.showMusicArtists"
      :show-music-album="settings.showMusicAlbum"
      :lyric-lines="lyricLines"
      :active-line-index="activeLineIndex"
      :show-translation="settings.enableLyricTranslationLine"
      :font-size-px="fontSizePx"
      :font-weight="settings.lyricFontWeight"
      :letter-spacing="settings.lyricLetterSpacing"
      :font-family="settings.lyricFontFamily"
      :enable-blur="settings.enableLyricLineBlurEffect"
      :enable-scale="settings.enableLyricLineScaleEffect"
      :enable-spring="settings.enableLyricLineSpringAnimation"
      :duration-ms="durationMs"
      :buffer-ms="bufferMs"
      :formatted-current-time="formattedCurrentTime"
      :formatted-duration="formattedDuration"
      :formatted-remaining="formattedRemaining"
      :is-playing="isPlaying"
      :is-loading="isLoading"
      :is-shuffle="isShuffle"
      :is-repeat-all="isRepeatAll"
      :is-repeat-one="isRepeatOne"
      :volume="volume"
      :muted="muted"
      :show-volume-control="settings.showVolumeControl"
      :show-remaining-time="settings.showRemainingTime"
      :is-immersive="isImmersive"
      :hide-lyric="hideLyric"
      :immerse-cover="verticalImmerseCover"
      @close="closePlayerView"
      @open-menu="openTrackMenu"
      @seek="seek"
      @toggle-play="togglePlay"
      @prev="playPrevious"
      @next="playNext"
      @toggle-shuffle="toggleShuffleMode"
      @cycle-repeat="cycleRepeat"
      @toggle-mute="toggleMute"
      @volume="setVolume"
      @toggle-remaining="onToggleRemaining"
      @line-click="onLyricLineClick"
      @background-click="toggleImmersive"
    />

    <!-- 沉浸模式提示 -->
    <div v-if="isImmersive" class="chord-immersive-hint" @click="toggleImmersive">
      <HugeiconsIcon :icon="ExpandIcon" :size="20" />
    </div>
  </div>
</template>

<script setup>
/**
 * ChordPlayerView — 自研 Vue 播放器根容器（对齐 AMLL AutoLyricLayout）。
 *
 * # 布局
 * - AutoLyricLayout：用 ResizeObserver 检测宽 < 高 → 竖屏(VerticalLayout)，否则 → 横屏(HorizontalLayout)。
 * - 横屏：两列网格，左列 封面+信息+控件，右列 歌词，底部 切换按钮。
 * - 竖屏：顶部控制条 + 歌词（小封面左上），隐藏歌词 → 大封面居中 + 大控件（封面 FLIP 动画）。
 * - hideLyric（settings.hideLyricView）：独立于横/竖屏的用户开关。
 */
import { ref, computed, onMounted, onBeforeUnmount } from 'vue'
import { HugeiconsIcon } from '@hugeicons/vue'
import { ExpandIcon } from '@hugeicons/core-free-icons'
import { useChordPlayer } from '@/composables/useChordPlayer.js'
import { useChordAudioAnalyser } from './composables/useChordAudioAnalyser.js'
import ChordFluidBackground from './components/ChordFluidBackground.vue'
import ChordHorizontalLayout from './components/ChordHorizontalLayout.vue'
import ChordVerticalLayout from './components/ChordVerticalLayout.vue'

const p = useChordPlayer()
// 启动音频分析器并 provide 高频数据（低频音量/频谱由子组件注入）
useChordAudioAnalyser()

const {
  state, settings, activeLineIndex, lyricLines, coverUrl, bufferMs,
  isImmersive, title, artists, artistList, album, isPlaying, isLoading, durationMs,
  volume, muted, isShuffle, isRepeatAll, isRepeatOne,
  formattedCurrentTime, formattedDuration, formattedRemaining,
  togglePlay, playPrevious, playNext, seek, setVolume, toggleMute,
  cycleRepeat, toggleShuffleMode, closePlayerView, openTrackMenu,
  openPlayQueue, onLyricLineClick,
} = p

// 竖屏布局：宽 < 高（对齐 AMLL AutoLyricLayout 的 clientWidth < clientHeight）
const rootEl = ref(null)
const isVertical = ref(false)

// 隐藏歌词（用户开关，独立于横/竖屏）
const hideLyric = computed(() => !!settings.hideLyricView)

// 竖屏沉浸封面（对齐 AMLL verticalCoverLayout："force-immersive" 时强制大封面沉浸）
const verticalImmerseCover = computed(
  () => hideLyric.value && settings.verticalCoverLayout === 'force-immersive',
)

// 底部控件是否显示（AMLL showBottomControl + 非 none 类型）
const showBottomControls = computed(() => settings.showBottomControl)

// 音质标签（当前无数据源，留空）
const qualityLabel = computed(() => '')

// 字号档位 → px
const SIZE_PRESET_PX = {
  tiny: 18,
  'extra-small': 22,
  small: 26,
  medium: 30,
  large: 36,
  'extra-large': 44,
  huge: 52,
}
const fontSizePx = computed(() => SIZE_PRESET_PX[settings.lyricSizePreset] || 30)

// 沉浸模式：点击空白处切换
function toggleImmersive() {
  isImmersive.value = !isImmersive.value
}

// 剩余时间标签点击：在 剩余时间 / 总时长 间切换（对齐 AMLL）
function onToggleRemaining() {
  settings.showRemainingTime = !settings.showRemainingTime
}

// 隐藏/显示歌词
function toggleHideLyric() {
  settings.hideLyricView = !settings.hideLyricView
}

// 投放（暂无可投设备，留空实现）
function onAirplay() {}

let io = null

onMounted(() => {
  const el = rootEl.value
  if (!el) return
  isVertical.value = el.clientWidth < el.clientHeight
  io = new ResizeObserver(() => {
    const b = el.getBoundingClientRect()
    isVertical.value = b.width < b.height
  })
  io.observe(el)
})

onBeforeUnmount(() => io?.disconnect())
</script>

<style scoped>
.chord-player-view {
  position: relative;
  width: 100%;
  height: 100%;
  overflow: hidden;
  background: #111111;
  color: #fff;
  font-family: var(--font-sans, system-ui, sans-serif);
}

.chord-background {
  position: absolute;
  inset: 0;
  z-index: 0;
}

/* 沉浸模式提示 */
.chord-immersive-hint {
  position: absolute;
  z-index: 5;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  width: 48px;
  height: 48px;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.15);
  color: #fff;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  opacity: 0;
  animation: hint-in 0.3s forwards;
}

@keyframes hint-in {
  to { opacity: 1; }
}
</style>