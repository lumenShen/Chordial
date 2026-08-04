<script setup>
/**
 * TrackMenuSourceMatch — 来源匹配区（搜索网易云同名曲，预览/切换播放）
 */
import { useTrackMenu } from './useTrackMenu.js';

const {
  onlineMatches,
  onlineSearching,
  onlineSearchDone,
  searchOnlineMatches,
  playOnlineMatch,
} = useTrackMenu();
</script>

<template>
  <section class="tms-section">
    <div class="tms-section-header">
      <h4 class="tms-section-title">来源匹配</h4>
      <button
        class="tms-refresh-btn"
        :disabled="onlineSearching"
        @click="searchOnlineMatches"
      >
        <HIcon name="arrow-repeat" :class="{ 'tms-spin': onlineSearching }" />
        {{ onlineSearchDone ? '重新搜索' : '搜索网易云' }}
      </button>
    </div>

    <div v-if="onlineSearching" class="tms-loading">
      <HIcon name="arrow-repeat" class="tms-spin" /> 搜索中...
    </div>

    <div v-else-if="onlineSearchDone && onlineMatches.length === 0" class="tms-empty-inline">
      未找到匹配曲目
    </div>

    <div v-else-if="onlineMatches.length > 0" class="tms-match-list">
      <button
        v-for="m in onlineMatches"
        :key="m.sourceIds?.find((s) => s.sourceName === 'netease')?.entityId || m.id"
        class="tms-match-item"
        @click="playOnlineMatch(m)"
      >
        <div class="tms-match-info">
          <div class="tms-match-title" :title="m.title">{{ m.title }}</div>
          <div class="tms-match-sub">
            {{ m.artist || '未知歌手' }}
            <span v-if="m.albumTitle"> · {{ m.albumTitle }}</span>
          </div>
        </div>
        <HIcon name="play-circle" class="tms-match-play" />
      </button>
    </div>

    <div v-else class="tms-empty-inline">
      点击"搜索网易云"查找同名曲
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

.tms-section-title {
  font-size: var(--font-size-xs, 12px);
  font-weight: var(--font-bold, 600);
  color: var(--text-secondary, #666);
  text-transform: uppercase;
  letter-spacing: 0.5px;
  margin: 0;
}

.tms-section-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.tms-refresh-btn {
  border: none;
  background: var(--bg-tertiary, rgba(15, 15, 15, 0.05));
  color: var(--text-secondary, #666);
  font-size: var(--font-size-xs, 12px);
  padding: 4px 10px;
  border-radius: var(--radius-full, 999px);
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  gap: 4px;
}

.tms-refresh-btn:hover:not(:disabled) {
  background: var(--bg-hover, rgba(15, 15, 15, 0.08));
}

.tms-refresh-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.tms-spin {
  animation: tms-spin 0.9s linear infinite;
  display: inline-block;
}

@keyframes tms-spin {
  to { transform: rotate(360deg); }
}

/* ── Match list (online search) ────────────────── */
.tms-loading,
.tms-empty-inline {
  padding: var(--space-2, 8px) 0;
  color: var(--text-tertiary, #999);
  font-size: var(--font-size-sm, 13px);
  display: flex;
  align-items: center;
  gap: 6px;
}

.tms-match-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
  max-height: 280px;
  overflow-y: auto;
}

.tms-match-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  border: none;
  background: transparent;
  color: var(--text-primary, #333);
  border-radius: var(--radius-sm, 6px);
  cursor: pointer;
  text-align: left;
  width: 100%;
}

.tms-match-item:hover {
  background: var(--bg-hover, rgba(15, 15, 15, 0.06));
}

.tms-match-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.tms-match-title {
  font-size: var(--font-size-sm, 13px);
  color: var(--text-primary, #333);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tms-match-sub {
  font-size: var(--font-size-xs, 11px);
  color: var(--text-tertiary, #999);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tms-match-play {
  font-size: 18px;
  color: var(--text-secondary, #666);
  flex-shrink: 0;
}

/* ── Dark mode ─────────────────────────────────── */
@media (prefers-color-scheme: dark) {
  .tms-match-item:hover {
    background: rgba(255, 255, 255, 0.08);
  }
}
</style>