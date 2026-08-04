<script setup>
/**
 * PlaylistCard — 单个歌单结果卡片
 */
defineProps({
  item: { type: Object, required: true },
  saved: { type: Boolean, default: false },
  formatPlayCount: { type: Function, default: null },
});

const emit = defineEmits(['click']);
</script>

<template>
  <div class="result-card" @click="emit('click', item)">
    <div class="card-cover">
      <img
        v-if="item.coverImgUrl"
        :src="item.coverImgUrl"
        :alt="item.name"
        loading="lazy"
        @error="(e) => e.target.style.display = 'none'"
      />
      <div v-else class="cover-placeholder">
        <HIcon name="music-note-list" />
      </div>
      <div v-if="saved" class="saved-badge" title="已保存到本地">
        <HIcon name="check-circle-fill" />
      </div>
    </div>
    <div class="card-info">
      <h3 class="card-title" :title="item.name">{{ item.name }}</h3>
      <p class="card-meta">
        <span v-if="item.creator">{{ item.creator.nickname }}</span>
        <span class="dot">·</span>
        <span>{{ item.trackCount }} 首</span>
        <template v-if="item.playCount">
          <span class="dot">·</span>
          <span>{{ formatPlayCount ? formatPlayCount(item.playCount) : item.playCount }} 次播放</span>
        </template>
      </p>
      <p v-if="item.description" class="card-desc">{{ item.description }}</p>
    </div>
  </div>
</template>

<style scoped>
.result-card {
  cursor: pointer;
  border-radius: 12px;
  background: var(--bg-secondary, #fff);
  border: 1px solid var(--border-light, #e8e8e8);
  overflow: hidden;
  transition: all 0.2s ease;
}

.result-card:hover {
  transform: translateY(-2px);
  border-color: var(--primary-color);
  box-shadow: 0 6px 20px rgba(0, 0, 0, 0.08);
}

.card-cover {
  position: relative;
  aspect-ratio: 1;
  background: var(--bg-hover, rgba(0, 0, 0, 0.04));
  overflow: hidden;
}

.card-cover img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.cover-placeholder {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 36px;
  color: var(--text-tertiary, #999);
}

.saved-badge {
  position: absolute;
  top: 8px;
  right: 8px;
  width: 24px;
  height: 24px;
  border-radius: 50%;
  background: rgba(40, 167, 69, 0.9);
  color: #fff;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 12px;
  backdrop-filter: blur(8px);
}

.card-info {
  padding: 10px 12px 12px;
}

.card-title {
  font-size: 14px;
  font-weight: 600;
  margin: 0 0 4px;
  color: var(--text-primary, #333);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.card-meta {
  font-size: 12px;
  color: var(--text-secondary, #666);
  margin: 0 0 2px;
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px;
}

.card-meta .dot {
  color: var(--text-tertiary, #999);
}

.card-desc {
  font-size: 11px;
  color: var(--text-tertiary, #999);
  margin: 4px 0 0;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
</style>