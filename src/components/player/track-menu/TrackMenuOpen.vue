<script setup>
/**
 * TrackMenuOpen — 打开区（本地目录 / 在线页面）
 */
import { useTrackMenu } from './useTrackMenu.js';

const {
  canRevealInFolder,
  canOpenOnline,
  revealInFolder,
  openOnlinePage,
} = useTrackMenu();
</script>

<template>
  <section class="tms-section">
    <h4 class="tms-section-title">打开</h4>
    <div class="tms-actions">
      <button
        class="tms-action-btn"
        :disabled="!canRevealInFolder"
        @click="revealInFolder"
      >
        <HIcon name="folder2-open" />
        <span>在文件管理器中显示</span>
      </button>
      <button
        class="tms-action-btn"
        :disabled="!canOpenOnline"
        @click="openOnlinePage"
      >
        <HIcon name="box-arrow-up-right" />
        <span>打开在线页面</span>
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

/* ── Dark mode ─────────────────────────────────── */
@media (prefers-color-scheme: dark) {
  .tms-action-btn {
    background: rgba(255, 255, 255, 0.04);
    color: var(--text-primary, #f6f6f6);
  }
  .tms-action-btn:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.08);
  }
}
</style>