<template>
  <div class="chord-bottom-controls" :class="{ 'is-immersive': isImmersive }">
    <!-- 进度行 -->
    <div class="bc-progress-row">
      <span class="bc-time">{{ formattedCurrentTime }}</span>
      <ChordProgressBar
        :duration-ms="durationMs"
        :buffer-ms="bufferMs"
        @seek="onSeek"
      />
      <span class="bc-time bc-remaining">
        {{ showRemainingTime ? formattedRemaining : formattedDuration }}
      </span>
    </div>

    <!-- 控制按钮行 -->
    <div class="bc-buttons">
      <div class="bc-left">
        <button
          class="bc-btn"
          :class="{ active: isShuffle }"
          title="随机播放"
          @click="onToggleShuffle"
        >
          <HugeiconsIcon :icon="ShuffleIcon" :size="20" />
        </button>
        <button
          class="bc-btn"
          :class="{ active: isRepeatAll || isRepeatOne }"
          :title="repeatTitle"
          @click="onCycleRepeat"
        >
          <HugeiconsIcon :icon="repeatIcon" :size="20" />
        </button>
      </div>

      <div class="bc-center">
        <button class="bc-btn" title="上一首" @click="onPrev">
          <HugeiconsIcon :icon="ArrowTurnBackwardIcon" :size="22" />
        </button>
        <button class="bc-btn bc-play" title="播放/暂停" @click="onTogglePlay">
          <span v-if="isLoading" class="bc-spinner" />
          <HugeiconsIcon v-else :icon="isPlaying ? PauseIcon : PlayIcon" :size="30" />
        </button>
        <button class="bc-btn" title="下一首" @click="onNext">
          <HugeiconsIcon :icon="FastForwardIcon" :size="22" />
        </button>
      </div>

      <div class="bc-right">
        <!-- 音量 -->
        <div class="bc-volume" v-if="showVolumeControl">
          <button class="bc-btn" title="音量" @click="onToggleMute">
            <HugeiconsIcon :icon="muted ? VolumeMute01Icon : VolumeUpIcon" :size="20" />
          </button>
          <input
            class="bc-volume-slider"
            type="range"
            min="0"
            max="1"
            step="0.01"
            :value="muted ? 0 : volume"
            @input="onVolumeInput"
          />
        </div>
        <button class="bc-btn" title="播放队列" @click="onOpenQueue">
          <HugeiconsIcon :icon="Queue01Icon" :size="20" />
        </button>
      </div>
    </div>
  </div>
</template>

<script setup>
/**
 * ChordBottomControls — 底部控制条。
 * 进度条 + 时间 + 播放模式 + 上一首/播放/下一首 + 音量 + 播放队列。
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
  Queue01Icon,
} from '@hugeicons/core-free-icons'
import ChordProgressBar from './ChordProgressBar.vue'

const props = defineProps({
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
  'toggle-shuffle', 'cycle-repeat', 'toggle-mute', 'volume', 'open-queue',
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
function onOpenQueue() { emit('open-queue') }
</script>

<style scoped>
.chord-bottom-controls {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 8px 20px 20px;
  user-select: none;
  transition: opacity 0.3s, transform 0.3s;
}

.chord-bottom-controls.is-immersive {
  opacity: 0;
  pointer-events: none;
  transform: translateY(12px);
}

.bc-progress-row {
  display: flex;
  align-items: center;
  gap: 12px;
}

.bc-time {
  font-size: 12px;
  color: rgba(255, 255, 255, 0.75);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
  min-width: 40px;
}

.bc-remaining {
  text-align: right;
}

.bc-buttons {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.bc-left,
.bc-right {
  display: flex;
  align-items: center;
  gap: 4px;
  flex: 1;
}

.bc-right {
  justify-content: flex-end;
}

.bc-center {
  display: flex;
  align-items: center;
  gap: 16px;
}

.bc-btn {
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

.bc-btn:hover {
  background: rgba(255, 255, 255, 0.12);
  color: #fff;
}

.bc-btn:active {
  transform: scale(0.92);
}

.bc-btn.active {
  color: #fff;
}

.bc-play {
  width: 52px;
  height: 52px;
  background: rgba(255, 255, 255, 0.16);
  color: #fff;
}

.bc-play:hover {
  background: rgba(255, 255, 255, 0.24);
}

.bc-spinner {
  width: 26px;
  height: 26px;
  border: 3px solid rgba(255, 255, 255, 0.4);
  border-top-color: #fff;
  border-radius: 50%;
  animation: bc-spin 0.8s linear infinite;
}

@keyframes bc-spin {
  to { transform: rotate(360deg); }
}

.bc-volume {
  display: flex;
  align-items: center;
  gap: 4px;
}

.bc-volume-slider {
  width: 0;
  opacity: 0;
  transition: width 0.25s, opacity 0.25s;
  accent-color: #fff;
  cursor: pointer;
}

.bc-volume:hover .bc-volume-slider {
  width: 80px;
  opacity: 1;
}

@media (max-width: 767px) {
  .bc-volume-slider {
    display: none;
  }
}
</style>