<script setup>
/**
 * HomeAlbumCard — "热门专辑"网格里的单张专辑卡片
 *
 * 职责：
 *  - 展示封面 + 标题 + 歌手
 *  - 悬停展示播放遮罩
 *  - 点击卡片跳转专辑详情
 */
import CoverImage from '../common/CoverImage.vue';

defineProps({
  album: { type: Object, required: true },
});

const emit = defineEmits(['select']);
</script>

<template>
  <div class="album-card" @click="emit('select', album)">
    <div class="album-cover-wrapper">
      <CoverImage
        :item="album"
        :alt="album.title"
        type="album"
        priority
      />
      <div class="album-overlay">
        <button class="album-play-btn">
          <HIcon name="play-fill" />
        </button>
      </div>
    </div>
    <div class="album-info">
      <h3 class="album-title">{{ album.title || '未知专辑' }}</h3>
      <p class="album-artist">{{ album.artist || '未知歌手' }}</p>
    </div>
  </div>
</template>

<style scoped>
.album-card {
  cursor: pointer;
  transition: transform var(--transition-spring);
}

.album-card:hover {
  transform: translateY(-8px);
}

.album-card:hover .album-overlay {
  opacity: 1;
}

.album-cover-wrapper {
  position: relative;
  aspect-ratio: 1;
  border-radius: var(--radius-lg);
  overflow: hidden;
  margin-bottom: 14px;
  box-shadow: var(--shadow-sm);
  transition: box-shadow var(--transition-normal);
}

.album-card:hover .album-cover-wrapper {
  box-shadow: var(--shadow-md);
}

.album-cover-wrapper :deep(.cover-wrapper) {
  width: 100%;
  height: 100%;
}

.album-overlay {
  position: absolute;
  inset: 0;
  background: rgba(0, 0, 0, 0.4);
  display: flex;
  align-items: center;
  justify-content: center;
  opacity: 0;
  transition: opacity var(--transition-fast);
}

.album-play-btn {
  width: 56px;
  height: 56px;
  border-radius: 50%;
  background: var(--primary-color);
  color: white;
  border: none;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  transition: transform var(--transition-fast);
  box-shadow: var(--shadow-primary);
}

.album-play-btn:hover {
  transform: scale(1.1);
}

.album-play-btn i {
  font-size: 24px;
  margin-left: 2px;
}

.album-info {
  padding: 0 4px;
}

.album-title {
  font-size: 15px;
  font-weight: 600;
  color: var(--text-primary);
  margin: 0 0 5px 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  letter-spacing: -0.2px;
}

.album-artist {
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
  .album-play-btn {
    width: 44px;
    height: 44px;
  }

  .album-play-btn i {
    font-size: 20px;
  }
}
</style>