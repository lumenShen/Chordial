<script setup>
/**
 * HomeTrackCard — "为你推荐"横向滚动里的单张曲目卡片
 *
 * 职责：
 *  - 展示封面 + 标题 + 歌手
 *  - 悬停播放按钮
 *  - 点击卡片选中 / 点击播放按钮播放
 */
import CoverImage from '../common/CoverImage.vue';

defineProps({
  track: { type: Object, required: true },
});

const emit = defineEmits(['select', 'play']);
</script>

<template>
  <div class="track-card" @click="emit('select', track)">
    <div class="track-card-cover">
      <CoverImage
        :item="track"
        :alt="track.title"
        type="track"
        priority
      />
      <button class="track-card-play" @click.stop="emit('play', track)">
        <HIcon name="play-fill" />
      </button>
    </div>
    <div class="track-card-info">
      <h3 class="track-card-title">{{ track.title || track.fileName || '未知歌曲' }}</h3>
      <p class="track-card-artist">{{ track.getDisplayArtist ? track.getDisplayArtist() : (track.artist || '未知歌手') }}</p>
    </div>
  </div>
</template>

<style scoped>
.track-card {
  flex: 0 0 180px;
  cursor: pointer;
  transition: transform var(--transition-spring);
}

.track-card:hover {
  transform: translateY(-8px);
}

.track-card:hover .track-card-play {
  opacity: 1;
  transform: scale(1);
}

.track-card-cover {
  position: relative;
  aspect-ratio: 1;
  border-radius: var(--radius-lg);
  overflow: hidden;
  margin-bottom: 12px;
  box-shadow: var(--shadow-sm);
  transition: box-shadow var(--transition-normal);
}

.track-card:hover .track-card-cover {
  box-shadow: var(--shadow-md);
}

.track-card-cover :deep(.cover-wrapper) {
  width: 100%;
  height: 100%;
}

.track-card-play {
  position: absolute;
  bottom: 12px;
  right: 12px;
  width: 44px;
  height: 44px;
  border-radius: 50%;
  background: var(--primary-color);
  color: white;
  border: none;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  opacity: 0;
  transform: scale(0.8);
  transition: all var(--transition-fast);
  box-shadow: var(--shadow-primary);
}

.track-card-play:hover {
  transform: scale(1.1) !important;
}

.track-card-play i {
  font-size: 20px;
  margin-left: 2px;
}

.track-card-info {
  padding: 0 4px;
}

.track-card-title {
  font-size: 15px;
  font-weight: 600;
  color: var(--text-primary);
  margin: 0 0 4px 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  letter-spacing: -0.2px;
}

.track-card-artist {
  font-size: 13px;
  color: var(--text-secondary);
  margin: 0;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* 响应式 */
@media (max-width: 767px) {
  .track-card {
    flex: 0 0 150px;
  }

  .track-card-play {
    width: 44px;
    height: 44px;
    opacity: 1;
    transform: scale(1);
    bottom: 8px;
    right: 8px;
  }

  .track-card-play i {
    font-size: 16px;
  }
}
</style>