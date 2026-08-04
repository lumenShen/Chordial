<template>
  <div class="player-control-bar" ref="root">
    <LiquidGlass
      :corner-radius="38"
      :elasticity="0.2"
      padding="0.5rem 1rem"
      extra-class="bar-glass"
    >
      <!-- 模糊封面背景层 -->
      <div class="bar-bg" v-if="albumCoverUrl">
        <img :src="albumCoverUrl" alt="" />
      </div>

      <div class="controls-section">
        <!-- 专辑封面 + 歌曲信息（真共享 DOM：动态 Teleport 到 PlayerView portal） -->
        <PlayerNowPlaying
          variant="compact"
          :cover-url="albumCoverUrl"
          :title="trackTitle"
          :meta="trackMeta"
        />

        <button class="control-btn play-btn" @click="togglePlay">
          <HugeiconsIcon :icon="isPlaying ? PauseIcon : PlayIcon" :size="26" />
        </button>

        <button class="control-btn" @click="$emit('next')">
          <HugeiconsIcon :icon="FastForwardIcon" :size="20" />
        </button>
      </div>
    </LiquidGlass>
  </div>
</template>

<script setup>
/**
 * PlayerControlBar - 播放器控制栏（移动端悬浮控制条）
 *
 * 提供播放控制、歌曲信息展示、封面模糊背景等功能。
 * 从 PlayerStore 读取当前播放状态和歌曲元数据。
 *
 * 封面 + 元信息使用 PlayerNowPlaying 共享组件（真共享 DOM Teleport），
 * 与桌面端 AppBottomBar 保持一致，避免重复。
 */
import { computed, onMounted, useTemplateRef } from 'vue'
import { HugeiconsIcon } from '@hugeicons/vue'
import {
  PlayIcon,
  PauseIcon,
  FastForwardIcon,
} from '@hugeicons/core-free-icons'
import { PlayerStore } from '@/stores/player/index.js'
import { usePerf } from '@/utils/performanceMonitor.js'
import { useAnime } from '@/composables/useAnime.js'
import LiquidGlass from '@/components/ui/LiquidGlass.vue'
import PlayerNowPlaying from './PlayerNowPlaying.vue'
const { log } = usePerf('PlayerControlBar')

const rootRef = useTemplateRef('root')
const { run } = useAnime(() => rootRef.value)

// 入场动画：控制栏向上淡入
onMounted(() => {
  run(({ animate, presets }) => {
    animate(rootRef.value, { ...presets.fadeInUp })
  })
})

const props = defineProps({
  isPlaying: {
    type: Boolean,
    default: false
  },
  albumCoverUrl: {
    type: String,
    default: null
  }
})

const emit = defineEmits(['play', 'pause', 'next'])

const trackTitle = computed(() => PlayerStore.state.currentTrack?.title ?? '')
const trackArtist = computed(() => PlayerStore.state.currentTrack?.artist ?? '')
const trackAlbum  = computed(() => PlayerStore.state.currentTrack?.albumTitle ?? '')

const trackMeta = computed(() => {
  const parts = [trackArtist.value, trackAlbum.value].filter(Boolean)
  return parts.join(' · ')
})

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
</script>

<style scoped>
.player-control-bar {
  display: flex;
  flex-direction: column;
  position: fixed;
  bottom: calc(1.5rem + var(--safe-area-bottom));
  right: 0.9rem;
  transition: bottom var(--transition-slow);
  justify-content: center;
}

.bar-glass {
  height: var(--bottom-nav-height);
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  isolation: isolate;
  position: relative;
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

/* 有封面背景时切换为白色文字 */
.bar-bg ~ .controls-section .control-btn {
  color: rgba(255, 255, 255, 0.92);
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

.control-btn:hover {
  opacity: 1;
}

.play-btn {
  font-size: 1.75rem;
}

@media (max-width: 767px) {
  .player-control-bar {
    right: 1rem;
    left: 1rem;
  }

  .controls-section {
    gap: 0.5rem;
  }
}

@media (prefers-color-scheme: dark) {
  .player-control-bar {
    background: rgba(40, 40, 45, 0.5);
    border-color: rgba(255, 255, 255, 0.15);
    box-shadow:
      0 8px 24px rgba(0, 0, 0, 0.35),
      inset 0 1px 0 rgba(255, 255, 255, 0.1);
  }
}
</style>