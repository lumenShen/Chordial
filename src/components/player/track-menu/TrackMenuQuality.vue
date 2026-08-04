<script setup>
/**
 * TrackMenuQuality — 音质信息区（基于文件扩展名推断）
 */
import { useTrackMenu } from './useTrackMenu.js';

const { currentQuality } = useTrackMenu();
</script>

<template>
  <section v-if="currentQuality" class="tms-section">
    <h4 class="tms-section-title">音质信息</h4>
    <div class="tms-quality">
      <div class="tms-quality-item">
        <span class="tms-quality-label">来源</span>
        <span class="tms-quality-value">{{ currentQuality.sourceName }}</span>
      </div>
      <div class="tms-quality-item">
        <span class="tms-quality-label">编码</span>
        <span class="tms-quality-value">{{ currentQuality.codec }}</span>
      </div>
      <div class="tms-quality-item">
        <span class="tms-quality-label">等级</span>
        <span class="tms-quality-value">{{ currentQuality.tier }}</span>
      </div>
      <div v-if="currentQuality.ext" class="tms-quality-item">
        <span class="tms-quality-label">扩展名</span>
        <span class="tms-quality-value">.{{ currentQuality.ext }}</span>
      </div>
      <div class="tms-quality-item">
        <span class="tms-quality-label">类型</span>
        <span class="tms-quality-value">
          {{ currentQuality.isLocal ? '本地文件' : '在线流' }}
        </span>
      </div>
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

/* ── Quality info ──────────────────────────────── */
.tms-quality {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: var(--space-2, 8px);
}

.tms-quality-item {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 8px 10px;
  background: var(--bg-secondary, #FBFBFB);
  border-radius: var(--radius-sm, 8px);
}

.tms-quality-label {
  font-size: var(--font-size-xs, 11px);
  color: var(--text-tertiary, #999);
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.tms-quality-value {
  font-size: var(--font-size-sm, 13px);
  color: var(--text-primary, #333);
  font-weight: 500;
}

/* ── Dark mode ─────────────────────────────────── */
@media (prefers-color-scheme: dark) {
  .tms-quality-item {
    background: rgba(255, 255, 255, 0.04);
    color: var(--text-primary, #f6f6f6);
  }
}
</style>