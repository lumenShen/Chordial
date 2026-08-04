<template>
  <div class="chord-player-view" :class="{ 'is-immersive': isImmersive }">
    <!-- 流体背景层 -->
    <ChordFluidBackground
      :cover-url="coverUrl"
      :static-mode="settings.lyricBackgroundStaticMode"
      :fps="settings.lyricBackgroundFPS"
      :vsync="settings.lyricBackgroundVSync"
      :render-scale="settings.lyricBackgroundRenderScale"
    />

    <!-- 内容层 -->
    <div class="chord-content">
      <!-- 顶部控制横条（横屏布局） -->
      <ChordControlThumb
        v-if="!isVerticalLayout"
        :title="title"
        :artists="artists"
        :artist-list="artistList"
        :album="album"
        :quality-label="qualityLabel"
        :show-music-name="settings.showMusicName"
        :show-music-artists="settings.showMusicArtists"
        :show-music-album="settings.showMusicAlbum"
        :is-immersive="isImmersive"
        @close="closePlayerView"
        @open-menu="openTrackMenu"
      />

      <!-- 主体：歌词 或 封面（竖屏/隐藏歌词） -->
      <div class="chord-main">
        <template v-if="isVerticalLayout">
          <ChordCoverPanel :cover-url="coverUrl" :music-paused="!isPlaying" />
        </template>
        <template v-else>
          <ChordLyricsView
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
            @line-click="onLyricLineClick"
            @background-click="toggleImmersive"
          />
        </template>
      </div>

      <!-- 底部：FFT 可视化 或 控制条 -->
      <template v-if="showBottomControls">
      <ChordFftControls
        v-if="controlsType === 'fft'"
        :is-immersive="isImmersive"
      />
      <ChordBottomControls
        v-else-if="controlsType === 'controls'"
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
        @seek="seek"
        @toggle-play="togglePlay"
        @prev="playPrevious"
        @next="playNext"
        @toggle-shuffle="toggleShuffleMode"
        @cycle-repeat="cycleRepeat"
        @toggle-mute="toggleMute"
        @volume="setVolume"
        @open-queue="openPlayQueue"
        @toggle-remaining="onToggleRemaining"
      />
      </template>
    </div>

    <!-- 沉浸模式提示（点击空白处退出） -->
    <div v-if="isImmersive" class="chord-immersive-hint" @click="toggleImmersive">
      <HugeiconsIcon :icon="ExpandIcon" :size="20" />
    </div>
  </div>
</template>

<script setup>
/**
 * ChordPlayerView — 自研 Vue 播放器根容器。
 *
 * 组装：流体背景 + 顶部控制横条 + 歌词/封面 + 底部控件，
 * 由 useChordPlayer 提供状态与 actions，由 useChordAudioAnalyser 提供音频分析数据。
 *
 * # 布局
 * - 横屏（默认）：顶部控制条 + 歌词 + 底部控件。
 * - 竖屏（hideLyricView）：顶部控制条 + 封面面板 + 底部控件。
 * - playerControlsType：controls（控制条）/ fft（频谱可视化）/ none（不显示）。
 */
import { computed, ref } from 'vue'
import { HugeiconsIcon } from '@hugeicons/vue'
import { ExpandIcon } from '@hugeicons/core-free-icons'
import { useChordPlayer } from '@/composables/useChordPlayer.js'
import { useChordAudioAnalyser } from './composables/useChordAudioAnalyser.js'
import ChordFluidBackground from './components/ChordFluidBackground.vue'
import ChordControlThumb from './components/ChordControlThumb.vue'
import ChordLyricsView from './components/ChordLyricsView.vue'
import ChordCoverPanel from './components/ChordCoverPanel.vue'
import ChordBottomControls from './components/ChordBottomControls.vue'
import ChordFftControls from './components/ChordFftControls.vue'

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

// 竖屏布局：隐藏歌词时显示封面
const isVerticalLayout = computed(() => !!settings.hideLyricView)

// 底部控件类型
const controlsType = computed(() => settings.playerControlsType)

// 底部控件是否显示（AMLL showBottomControl + 非 none 类型）
const showBottomControls = computed(() => settings.showBottomControl && controlsType.value !== 'none')

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

// 音质标签（当前无数据源，留空）
const qualityLabel = computed(() => '')

// 沉浸模式：点击空白处切换
function toggleImmersive() {
  isImmersive.value = !isImmersive.value
}

// 剩余时间标签点击：在 剩余时间 / 总时长 间切换（对齐 AMLL）
function onToggleRemaining() {
  settings.showRemainingTime = !settings.showRemainingTime
}
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

.chord-content {
  position: relative;
  z-index: 1;
  display: flex;
  flex-direction: column;
  height: 100%;
  width: 100%;
}

.chord-main {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
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