<template>
  <!-- 正在播放：封面 + 元信息（共享 DOM 槽位，勿动 Teleport 结构） -->
  <div class="player-now-playing" :class="`variant-${variant}`">
    <div class="cover-slot">
      <Teleport :to="coverPortalTarget" :disabled="!coverPortalTarget">
        <button
          class="cover-thumb"
          :class="{ placeholder: !coverUrl }"
          data-shared="cover"
          @click="openPlayerView"
          :title="title"
        >
          <img v-if="coverUrl" :src="coverUrl" alt="专辑封面" />
          <HugeiconsIcon v-else :icon="MusicNote01Icon" :size="coverIconSize" class="cover-ph-icon" />
        </button>
      </Teleport>
    </div>

    <div class="meta-slot">
      <Teleport :to="metaPortalTarget" :disabled="!metaPortalTarget">
        <div class="track-meta" data-shared="meta">
          <span class="track-title" :title="title">{{ title }}</span>
          <span class="track-artist" :title="meta">{{ meta }}</span>
        </div>
      </Teleport>
    </div>
  </div>
</template>

<script setup>
/**
 * PlayerNowPlaying - 正在播放的封面 + 元信息（共享 DOM 槽位）。
 *
 * 桌面端底栏（AppBottomBar）与移动端控制条（PlayerControlBar）共用同一套
 * `data-shared="cover" / "meta"` Teleport 结构，保证 PlayerView 打开时的
 * "共享 DOM" FLIP 动画一致生效。抽取为单一组件，避免两处重复。
 *
 * - `variant="full"`：桌面端底栏（封面 44px，元信息最大 340px）
 * - `variant="compact"`：移动端浮动控制条（封面 2.5rem，元信息最大 12rem）
 */
import { computed } from 'vue';
import { HugeiconsIcon } from '@hugeicons/vue';
import { MusicNote01Icon } from '@hugeicons/core-free-icons';
import PlayerStore from '@/stores/player/index.js';

const props = defineProps({
  coverUrl: { type: String, default: null },
  title: { type: String, default: '未在播放' },
  meta: { type: String, default: '选择一首歌曲开始播放' },
  variant: { type: String, default: 'full' }, // 'full' | 'compact'
});

// 真共享 DOM：动态 Teleport 目标（PlayerView 打开时设为 portal 容器）
const coverPortalTarget = computed(() => PlayerStore.coverPortalTarget.value);
const metaPortalTarget = computed(() => PlayerStore.metaPortalTarget.value);

const coverIconSize = computed(() => (props.variant === 'compact' ? 20 : 18));

const openPlayerView = () => PlayerStore.openPlayerView();
</script>

<style scoped>
.player-now-playing {
  display: flex;
  align-items: center;
  gap: 12px;
  min-width: 0;
}

/* 封面 */
.cover-slot {
  width: 44px;
  height: 44px;
  flex-shrink: 0;
}

.cover-thumb {
  width: 44px;
  height: 44px;
  border: none;
  border-radius: 6px;
  overflow: hidden;
  padding: 0;
  cursor: pointer;
  background: var(--bg-tertiary);
  display: flex;
  align-items: center;
  justify-content: center;
  box-shadow: var(--shadow-sm);
  transition: transform var(--transition-fast);
}

.cover-thumb:hover {
  transform: scale(1.04);
}

.cover-thumb img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
}

.cover-ph-icon {
  color: var(--text-tertiary);
}

/* 元信息 */
.meta-slot {
  min-width: 0;
  flex: 1;
  max-width: 340px;
}

.track-meta {
  display: flex;
  flex-direction: column;
  min-width: 0;
  line-height: 1.35;
}

.track-title {
  font-size: var(--font-size-md);
  font-weight: 600;
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.track-artist {
  font-size: var(--font-size-sm);
  color: var(--text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* ── compact 变体：移动端浮动控制条 ─────────────────── */
.variant-compact {
  gap: 0.75rem;
}

.variant-compact .cover-slot {
  width: 2.5rem;
  height: 2.5rem;
}

.variant-compact .cover-thumb {
  width: 2.5rem;
  height: 2.5rem;
  border-radius: 0.5rem;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.25);
}

.variant-compact .meta-slot {
  max-width: 12rem;
}

.variant-compact .track-title {
  font-size: 0.8125rem;
  line-height: 1.25;
}

.variant-compact .track-artist {
  font-size: 0.6875rem;
  margin-top: 1px;
  line-height: 1.25;
}

@media (max-width: 1100px) {
  .meta-slot {
    max-width: 240px;
  }
}

@media (max-width: 767px) {
  .variant-compact .meta-slot {
    max-width: none;
    flex: 1;
  }
}
</style>