<script setup>
/**
 * PlaylistSection - 侧边栏「播放列表」区段
 *
 * 展示歌单列表，支持新建歌单。当前路由匹配的条目高亮。
 */
defineProps({
  open: { type: Boolean, default: true },
  playlists: { type: Array, default: () => [] },
  activePath: { type: String, default: '' },
});

const emit = defineEmits(['toggle', 'create']);
</script>

<template>
  <section class="sidebar-section">
    <div class="section-title-row">
      <button class="section-title-btn grow" @click="emit('toggle')">
        <HIcon name="playlist" :size="15" class="title-icon" />
        <span>播放列表</span>
        <HIcon :name="open ? 'chevron-down' : 'chevron-right'" :size="14" class="title-chevron" />
      </button>
      <button class="section-add-btn" title="新建歌单" @click="emit('create')">
        <HIcon name="plus-circle" :size="15" />
      </button>
    </div>

    <div v-show="open" class="section-body">
      <div v-if="playlists.length === 0" class="section-empty">暂无歌单</div>
      <router-link
        v-for="pl in playlists"
        :key="pl.id"
        :to="`/playlists/${pl.id}`"
        class="playlist-item"
        :class="{ active: activePath === `/playlists/${pl.id}` }"
      >
        <span class="pl-cover">
          <img v-if="pl.cover_url" :src="pl.cover_url" :alt="pl.name" loading="lazy" />
          <HIcon v-else name="music-note-list" :size="15" class="pl-cover-ph" />
        </span>
        <span class="pl-name" :title="pl.name">{{ pl.name }}</span>
        <span class="pl-count">{{ pl.song_ids?.length ?? 0 }}</span>
      </router-link>
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

.section-add-btn {
  width: 26px;
  height: 26px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  border-radius: var(--radius-sm);
  transition: color var(--transition-fast), background var(--transition-fast);
}

.section-add-btn:hover {
  color: var(--text-primary);
  background: var(--bg-hover);
}

.section-body {
  padding: 2px 0 6px;
}

.section-empty {
  padding: 8px 10px;
  font-size: var(--font-size-sm);
  color: var(--text-tertiary);
}

/* ── 播放列表条目 ─────────────────────────────────────── */
.playlist-item {
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 6px 8px;
  border-radius: var(--radius-md);
  color: var(--text-primary);
  text-decoration: none;
  transition: background var(--transition-fast);
}

.playlist-item:hover {
  background: var(--bg-hover);
  text-decoration: none;
}

.playlist-item.active {
  background: var(--primary-light);
  color: var(--primary-color);
}

.pl-cover {
  width: 26px;
  height: 26px;
  border-radius: var(--radius-sm);
  background: var(--bg-tertiary);
  overflow: hidden;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
}

.pl-cover img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.pl-cover-ph {
  color: var(--text-tertiary);
}

.pl-name {
  flex: 1;
  min-width: 0;
  font-size: var(--font-size-base);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.pl-count {
  font-size: var(--font-size-xs);
  color: var(--text-tertiary);
  flex-shrink: 0;
}
</style>