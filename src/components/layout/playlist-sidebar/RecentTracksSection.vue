<script setup>
/**
 * RecentTracksSection - 侧边栏「最近播放」区段
 *
 * 展示最近播放记录，支持清空与点击播放。当前曲目高亮。
 */
defineProps({
  open: { type: Boolean, default: true },
  recentTracks: { type: Array, default: () => [] },
  currentTrackId: { type: [String, Number], default: null },
});

const emit = defineEmits(['toggle', 'clear', 'play']);
</script>

<template>
  <section class="sidebar-section">
    <div class="section-title-row">
      <button class="section-title-btn grow" @click="emit('toggle')">
        <HIcon name="clock" :size="15" class="title-icon" />
        <span>最近播放</span>
        <HIcon :name="open ? 'chevron-down' : 'chevron-right'" :size="14" class="title-chevron" />
      </button>
      <button
        v-if="recentTracks.length"
        class="section-clear-btn"
        title="清空最近播放"
        @click="emit('clear')"
      >
        清空
      </button>
    </div>

    <div v-show="open" class="section-body">
      <div v-if="recentTracks.length === 0" class="section-empty">暂无播放记录</div>
      <button
        v-for="t in recentTracks"
        :key="t.id"
        class="recent-item"
        :class="{ now: currentTrackId === t.id }"
        @click="emit('play', t)"
      >
        <span class="recent-title" :title="t.title">{{ t.title }}</span>
        <span class="recent-artist">{{ t.getDisplayArtist ? t.getDisplayArtist() : t.artist }}</span>
      </button>
    </div>
  </section>
</template>

<style scoped>
/* ── 区段 ─────────────────────────────────────────────── */
.sidebar-section {
  margin-bottom: 4px;
}

.section-title-row {
  display: flex;
  align-items: center;
  gap: 4px;
}

.section-title-btn {
  display: flex;
  align-items: center;
  gap: 7px;
  flex: 1;
  min-width: 0;
  padding: 6px 8px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--font-size-sm);
  font-weight: 600;
  letter-spacing: 0.03em;
  cursor: pointer;
  border-radius: var(--radius-sm);
  text-transform: uppercase;
  transition: color var(--transition-fast), background var(--transition-fast);
}

.section-title-btn.grow {
  flex: 1;
}

.section-title-btn:hover {
  color: var(--text-primary);
  background: var(--bg-hover);
}

.title-icon {
  color: var(--text-tertiary);
  flex-shrink: 0;
}

.title-chevron {
  color: var(--text-tertiary);
  margin-left: auto;
  flex-shrink: 0;
}

.section-clear-btn {
  border: none;
  background: transparent;
  color: var(--text-tertiary);
  font-size: var(--font-size-xs);
  cursor: pointer;
  padding: 4px 6px;
  border-radius: var(--radius-sm);
  transition: color var(--transition-fast);
}

.section-clear-btn:hover {
  color: var(--text-primary);
}

.section-body {
  padding: 2px 0 6px;
}

.section-empty {
  padding: 8px 10px;
  font-size: var(--font-size-sm);
  color: var(--text-tertiary);
}

/* ── 最近播放条目 ─────────────────────────────────────── */
.recent-item {
  display: flex;
  flex-direction: column;
  width: 100%;
  padding: 6px 8px;
  border: none;
  border-radius: var(--radius-md);
  background: transparent;
  color: var(--text-primary);
  cursor: pointer;
  text-align: left;
  transition: background var(--transition-fast);
}

.recent-item:hover {
  background: var(--bg-hover);
}

.recent-item.now {
  background: var(--primary-light);
}

.recent-item.now .recent-title {
  color: var(--primary-color);
}

.recent-title {
  font-size: var(--font-size-base);
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.recent-artist {
  font-size: var(--font-size-xs);
  color: var(--text-tertiary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>