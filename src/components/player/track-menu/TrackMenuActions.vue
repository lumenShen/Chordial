<script setup>
/**
 * TrackMenuActions — 歌曲操作区（添加到歌单 / 查看详情 + 歌单选择子列表 + 创建新歌单）
 */
import { useTrackMenu } from './useTrackMenu.js';

const {
  playlistPickerOpen,
  playlists,
  togglePlaylistPicker,
  addToPlaylist,
  createPlaylistAndAdd,
  viewTrackDetail,
} = useTrackMenu();
</script>

<template>
  <section class="tms-section">
    <h4 class="tms-section-title">歌曲操作</h4>
    <div class="tms-actions">
      <button class="tms-action-btn" @click="togglePlaylistPicker">
        <HIcon name="plus-square" />
        <span>添加到歌单</span>
      </button>
      <button class="tms-action-btn" @click="viewTrackDetail">
        <HIcon name="info-circle" />
        <span>查看详情</span>
      </button>
    </div>

    <!-- 歌单选择子列表 -->
    <div v-if="playlistPickerOpen" class="tms-sublist">
      <div v-if="playlists.length === 0" class="tms-sublist-empty">
        暂无歌单
      </div>
      <button
        v-for="pl in playlists"
        :key="pl.id"
        class="tms-sublist-item"
        @click="addToPlaylist(pl.id)"
      >
        <HIcon name="music-note-list" />
        <span class="tms-sublist-name">{{ pl.name }}</span>
        <span class="tms-sublist-count">{{ pl.song_ids?.length || 0 }} 首</span>
      </button>
      <button class="tms-sublist-item tms-sublist-create" @click="createPlaylistAndAdd">
        <HIcon name="plus-circle" />
        <span>创建新歌单</span>
      </button>
    </div>
  </section>
</template>

<style scoped>
/* ── Section ───────────────────────────────────── */
.tms-section {
  display: flex;
  flex-direction: column;
  gap: var(--space-2, 8px);
  padding: var(--space-2, 8px) 0;
  border-top: 1px solid var(--bg-tertiary, rgba(15, 15, 15, 0.05));
}

.tms-section:first-of-type {
  border-top: none;
}

.tms-section-title {
  font-size: var(--font-size-xs, 12px);
  font-weight: var(--font-bold, 600);
  color: var(--text-secondary, #666);
  text-transform: uppercase;
  letter-spacing: 0.5px;
  margin: 0;
}

/* ── Action buttons ────────────────────────────── */
.tms-actions {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: var(--space-2, 8px);
}

.tms-action-btn {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 12px;
  border: 1px solid var(--bg-tertiary, rgba(15, 15, 15, 0.08));
  background: var(--bg-secondary, #FBFBFB);
  color: var(--text-primary, #333);
  border-radius: var(--radius-md, 10px);
  font-size: var(--font-size-sm, 13px);
  cursor: pointer;
  transition: background 0.15s, transform 0.1s;
  text-align: left;
}

.tms-action-btn:hover:not(:disabled) {
  background: var(--bg-hover, rgba(15, 15, 15, 0.06));
}

.tms-action-btn:active:not(:disabled) {
  transform: scale(0.97);
}

.tms-action-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.tms-action-btn i {
  font-size: 16px;
  color: var(--text-secondary, #666);
  flex-shrink: 0;
}

/* ── Sublist (playlists) ───────────────────────── */
.tms-sublist {
  display: flex;
  flex-direction: column;
  gap: 2px;
  margin-top: var(--space-1, 4px);
}

.tms-sublist-empty {
  padding: var(--space-2, 8px);
  text-align: center;
  color: var(--text-tertiary, #999);
  font-size: var(--font-size-xs, 12px);
}

.tms-sublist-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  border: none;
  background: transparent;
  color: var(--text-primary, #333);
  border-radius: var(--radius-sm, 6px);
  font-size: var(--font-size-sm, 13px);
  cursor: pointer;
  text-align: left;
  width: 100%;
}

.tms-sublist-item:hover {
  background: var(--bg-hover, rgba(15, 15, 15, 0.06));
}

.tms-sublist-item i {
  color: var(--text-tertiary, #999);
  font-size: 14px;
  flex-shrink: 0;
}

.tms-sublist-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tms-sublist-count {
  font-size: var(--font-size-xs, 11px);
  color: var(--text-tertiary, #999);
  flex-shrink: 0;
}

.tms-sublist-create {
  color: var(--text-secondary, #666);
  font-style: italic;
  border-top: 1px dashed var(--bg-tertiary, rgba(15, 15, 15, 0.08));
  margin-top: 4px;
  padding-top: 10px;
}

/* ── Dark mode ─────────────────────────────────── */
@media (prefers-color-scheme: dark) {
  .tms-action-btn {
    background: rgba(255, 255, 255, 0.04);
    color: var(--text-primary, #f6f6f6);
  }
  .tms-action-btn:hover:not(:disabled),
  .tms-sublist-item:hover {
    background: rgba(255, 255, 255, 0.08);
  }
}
</style>