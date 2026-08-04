<script setup>
/**
 * TrackMenuHero — 歌曲菜单顶部曲目卡片（封面 + 标题/歌手/专辑）
 */
import { useTrackMenu } from './useTrackMenu.js';
import { useCoverImage } from '@/composables/useCoverImage';

const { track } = useTrackMenu();
const { coverUrl } = useCoverImage(track, 'small');
</script>

<template>
  <header class="tms-hero">
    <div class="tms-cover">
      <img v-if="coverUrl" :src="coverUrl" :alt="track.title" />
      <HIcon v-else name="music-note-beamed" class="tms-cover-placeholder" />
    </div>
    <div class="tms-meta">
      <div class="tms-title" :title="track.title">{{ track.title || '未知歌曲' }}</div>
      <div class="tms-artist" :title="track.artist">
        {{ track.artist || '未知歌手' }}
      </div>
      <div v-if="track.albumTitle" class="tms-album">
        <HIcon name="disc" />
        <span :title="track.albumTitle">{{ track.albumTitle }}</span>
      </div>
    </div>
  </header>
</template>

<style scoped>
/* ── Hero ──────────────────────────────────────── */
.tms-hero {
  display: flex;
  gap: var(--space-3, 12px);
  align-items: center;
  padding: var(--space-3, 12px) 0;
}

.tms-cover {
  width: 64px;
  height: 64px;
  border-radius: var(--radius-md, 10px);
  overflow: hidden;
  background: var(--bg-tertiary, rgba(15, 15, 15, 0.05));
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.tms-cover img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.tms-cover-placeholder {
  font-size: 28px;
  color: var(--text-tertiary, #999);
}

.tms-meta {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.tms-title {
  font-size: var(--font-size-md, 15px);
  font-weight: var(--font-bold, 600);
  color: var(--text-primary, #333);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tms-artist {
  font-size: var(--font-size-sm, 13px);
  color: var(--text-secondary, #666);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tms-album {
  font-size: var(--font-size-sm, 13px);
  color: var(--text-tertiary, #999);
  display: flex;
  align-items: center;
  gap: 4px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>