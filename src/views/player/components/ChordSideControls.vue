<template>
  <div class="chord-side-controls" :class="{ 'is-immersive': isImmersive }">
    <!-- 歌曲信息 -->
    <ChordMusicInfo
      :title="title"
      :artists="artists"
      :artist-list="artistList"
      :album="album"
      :show-music-name="showMusicName"
      :show-music-artists="showMusicArtists"
      :show-music-album="showMusicAlbum"
    />

    <!-- 进度行：当前时间 + 进度条 + 剩余/总时长 -->
    <div class="sc-progress-row">
      <span class="sc-time">{{ formattedCurrentTime }}</span>
      <ChordProgressBar
        :duration-ms="durationMs"
        :buffer-ms="bufferMs"
        @seek="onSeek"
      />
      <span
        class="sc-time sc-remaining"
        title="点击切换 剩余时间 / 总时长"
        @click="onToggleRemaining"
      >
        {{ showRemainingTime ? formattedRemaining : formattedDuration }}
      </span>
    </div>

    <!-- 传输控制行 -->
    <div class="sc-transport">
      <div class="sc-left">
        <button
          class="sc-btn"
          :class="{ active: isShuffle }"
          title="随机播放"
          @click="onToggleShuffle"
        >
          <HugeiconsIcon :icon="ShuffleIcon" :size="18" />
        </button>
        <button
          class="sc-btn"
          :class="{ active: isRepeatAll || isRepeatOne }"
          :title="repeatTitle"
          @click="onCycleRepeat"
        >
          <HugeiconsIcon :icon="repeatIcon" :size="18" />
        </button>
      </div>

      <div class="sc-center">
        <button class="sc-btn" title="上一首" @click="onPrev">
          <HugeiconsIcon :icon="ArrowTurnBackwardIcon" :size="20" />
        </button>
        <button class="sc-btn sc-play" title="播放/暂停" @click="onTogglePlay">
          <span v-if="isLoading" class="sc-spinner" />
          <HugeiconsIcon v-else :icon="isPlaying ? PauseIcon : PlayIcon" :size="26" />
        </button>
        <button class="sc-btn" title="下一首" @click="onNext">
          <HugeiconsIcon :icon="FastForwardIcon" :size="20" />
        </button>
      </div>

      <div class="sc-right">
        <div class="sc-volume" v-if="showVolumeControl">
          <button class="sc-btn" title="音量" @click="onToggleMute">
            <HugeiconsIcon :icon="muted ? VolumeMute01Icon : VolumeUpIcon" :size="18" />
          </button>
          <input
            class="sc-volume-slider"
            type="range"
            min="0"
            max="1"
            step="0.01"
            :value="muted ? 0 : volume"
            @input="onVolumeInput"
          />
        </div>
      </div>
    </div>
  </div>
</template>

<script setup>
/**
 * ChordSideControls — 横屏布局左侧列控件（对齐 AMLL Horizontal 的 controlsSlot）。
 * 组合：歌曲信息 + 进度条/时间 + 播放传输控制 + 音量。
 * 布局为纵向堆叠，位于封面下方、歌词左侧。
 */
import { computed } from 'vue'
import { HugeiconsIcon } from '@hugeicons/vue'
import {
  PlayIcon,
  PauseIcon,
  FastForwardIcon,
  ArrowTurnBackwardIcon,
  ShuffleIcon,
  RepeatIcon,
  RepeatOneIcon,
  RepeatOffIcon,
  VolumeUpIcon,
  VolumeMute01Icon,
} from '@hugeicons/core-free-icons'
import ChordMusicInfo from './ChordMusicInfo.vue'
import ChordProgressBar from './ChordProgressBar.vue'

const props = defineProps({
  title: { type: String, default: '未知歌曲' },
  artists: { type: String, default: '未知歌手' },
  artistList: { type: Array, default: () => [] },
  album: { type: String, default: '' },
  showMusicName: { type: Boolean, default: true },
  showMusicArtists: { type: Boolean, default: true },
  showMusicAlbum: { type: Boolean, default: false },
  durationMs: { type: Number, default: 0 },
  bufferMs: { type: Number, default: 0 },
  formattedCurrentTime: { type: String, default: '0:00' },
  formattedDuration: { type: String, default: '0:00' },
  formattedRemaining: { type: String, default: '0:00' },
  isPlaying: { type: Boolean, default: false },
  isLoading: { type: Boolean, default: false },
  isShuffle: { type: Boolean, default: false },
  isRepeatAll: { type: Boolean, default: false },
  isRepeatOne: { type: Boolean, default: false },
  volume: { type: Number, default: 0.8 },
  muted: { type: Boolean, default: false },
  showVolumeControl: { type: Boolean, default: true },
  showRemainingTime: { type: Boolean, default: true },
  isImmersive: { type: Boolean, default: false },
})

const emit = defineEmits([
  'seek', 'toggle-play', 'prev', 'next',
  'toggle-shuffle', 'cycle-repeat', 'toggle-mute', 'volume', 'toggle-remaining',
])

const repeatIcon = computed(() => {
  if (props.isRepeatOne) return RepeatOneIcon
  if (props.isRepeatAll) return RepeatIcon
  return RepeatOffIcon
})

const repeatTitle = computed(() => {
  if (props.isRepeatOne) return '单曲循环'
  if (props.isRepeatAll) return '列表循环'
  return '顺序播放'
})

function onSeek(ms) { emit('seek', ms) }
function onTogglePlay() { emit('toggle-play') }
function onPrev() { emit('prev') }
function onNext() { emit('next') }
function onToggleShuffle() { emit('toggle-shuffle') }
function onCycleRepeat() { emit('cycle-repeat') }
function onToggleMute() { emit('toggle-mute') }
function onVolumeInput(e) { emit('volume', parseFloat(e.target.value)) }
function onToggleRemaining() { emit('toggle-remaining') }
</script>

<style scoped>
.chord-side-controls {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-width: 0;
  user-select: none;
  transition: opacity 0.3s, transform 0.3s;
}

.chord-side-controls.is-immersive {
  opacity: 0;
  pointer-events: none;
}

.sc-progress-row {
  display: flex;
  align-items: center;
  gap: 10px;
}

.sc-time {
  font-size: 12px;
  color: rgba(255, 255, 255, 0.75);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
  min-width: 36px;
}

.sc-remaining {
  text-align: right;
  cursor: pointer;
  transition: color 0.2s;
}

.sc-remaining:hover {
  color: #fff;
}

.sc-transport {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.sc-left,
.sc-right {
  display: flex;
  align-items: center;
  gap: 2px;
  flex: 1;
}

.sc-right {
  justify-content: flex-end;
}

.sc-center {
  display: flex;
  align-items: center;
  gap: 12px;
}

.sc-btn {
  background: none;
  border: none;
  color: rgba(255, 255, 255, 0.85);
  cursor: pointer;
  padding: 6px;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: background 0.2s, color 0.2s, transform 0.1s;
}

.sc-btn:hover {
  background: rgba(255, 255, 255, 0.12);
  color: #fff;
}

.sc-btn:active {
  transform: scale(0.92);
}

.sc-btn.active {
  color: #fff;
}

.sc-play {
  width: 46px;
  height: 46px;
  background: rgba(255, 255, 255, 0.16);
  color: #fff;
}

.sc-play:hover {
  background: rgba(255, 255, 255, 0.24);
}

.sc-spinner {
  width: 22px;
  height: 22px;
  border: 3px solid rgba(255, 255, 255, 0.4);
  border-top-color: #fff;
  border-radius: 50%;
  animation: sc-spin 0.8s linear infinite;
}

@keyframes sc-spin {
  to { transform: rotate(360deg); }
}

.sc-volume {
  display: flex;
  align-items: center;
  gap: 4px;
}

.sc-volume-slider {
  width: 0;
  opacity: 0;
  transition: width 0.25s, opacity 0.25s;
  accent-color: #fff;
  cursor: pointer;
}

.sc-volume:hover .sc-volume-slider {
  width: 70px;
  opacity: 1;
}

@media (max-width: 767px) {
  .sc-volume-slider {
    display: none;
  }
}
</style>