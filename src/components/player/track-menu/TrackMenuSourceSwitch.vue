<script setup>
/**
 * TrackMenuSourceSwitch — 来源切换区（当前曲目的所有 sourceIds，选择后重排）
 */
import { useTrackMenu } from './useTrackMenu.js';

const {
  sourceIds,
  switching,
  switchSource,
  sourceDisplayName,
  sourceEntityShort,
} = useTrackMenu();
</script>

<template>
  <section class="tms-section">
    <h4 class="tms-section-title">
      播放来源
      <span class="tms-section-hint">（点击切换，当前来源排在最前）</span>
    </h4>

    <div v-if="sourceIds.length === 0" class="tms-empty-inline">
      该曲目无来源信息
    </div>

    <div v-else class="tms-source-list">
      <button
        v-for="(sid, idx) in sourceIds"
        :key="idx"
        class="tms-source-item"
        :class="{ 'tms-source-active': idx === 0 }"
        :disabled="idx === 0 || switching"
        @click="switchSource(sid)"
      >
        <div class="tms-source-main">
          <i
            class="bi"
            :class="sid.isLocal() ? 'bi-folder' : 'bi-cloud'"
          />
          <div class="tms-source-text">
            <div class="tms-source-name">{{ sourceDisplayName(sid) }}</div>
            <div class="tms-source-id" :title="sid.entityId">
              {{ sourceEntityShort(sid) }}
            </div>
          </div>
        </div>
        <HIcon v-if="idx === 0" name="check-circle-fill" class="tms-source-check" />
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

.tms-section-title {
  font-size: var(--font-size-xs, 12px);
  font-weight: var(--font-bold, 600);
  color: var(--text-secondary, #666);
  text-transform: uppercase;
  letter-spacing: 0.5px;
  margin: 0;
}

.tms-section-hint {
  font-size: var(--font-size-xs, 11px);
  font-weight: normal;
  color: var(--text-tertiary, #999);
  text-transform: none;
  letter-spacing: 0;
  margin-left: 6px;
}

.tms-empty-inline {
  padding: var(--space-2, 8px) 0;
  color: var(--text-tertiary, #999);
  font-size: var(--font-size-sm, 13px);
  display: flex;
  align-items: center;
  gap: 6px;
}

/* ── Source list ───────────────────────────────── */
.tms-source-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.tms-source-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 10px 12px;
  border: 1px solid transparent;
  background: var(--bg-secondary, #FBFBFB);
  color: var(--text-primary, #333);
  border-radius: var(--radius-md, 10px);
  cursor: pointer;
  text-align: left;
  width: 100%;
  transition: background 0.15s, border-color 0.15s;
}

.tms-source-item:hover:not(:disabled) {
  background: var(--bg-hover, rgba(15, 15, 15, 0.06));
}

.tms-source-item:disabled {
  cursor: not-allowed;
}

.tms-source-active {
  border-color: var(--primary-color);
  background: var(--bg-active);
}

.tms-source-main {
  display: flex;
  align-items: center;
  gap: 10px;
  flex: 1;
  min-width: 0;
}

.tms-source-main > i {
  font-size: 18px;
  color: var(--text-secondary, #666);
  flex-shrink: 0;
}

.tms-source-text {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.tms-source-name {
  font-size: var(--font-size-sm, 13px);
  font-weight: 500;
  color: var(--text-primary, #333);
}

.tms-source-id {
  font-size: var(--font-size-xs, 11px);
  color: var(--text-tertiary, #999);
  font-family: ui-monospace, "SF Mono", monospace;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tms-source-check {
  color: var(--primary-color);
  font-size: 16px;
  flex-shrink: 0;
}

/* ── Dark mode ─────────────────────────────────── */
@media (prefers-color-scheme: dark) {
  .tms-source-item {
    background: rgba(255, 255, 255, 0.04);
    color: var(--text-primary, #f6f6f6);
  }
  .tms-source-item:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.08);
  }
}
</style>