<script setup>
// 无 props / emits 的静态说明卡片
</script>

<template>
  <!-- Unsupported source notice -->
  <div class="unsupported-note card" style="margin-top: 24px;">
    <div class="note-content">
      <p class="note-title">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="note-icon">
          <circle cx="12" cy="12" r="10"/>
          <line x1="12" y1="8" x2="12" y2="12"/>
          <line x1="12" y1="16" x2="12.01" y2="16"/>
        </svg>
        关于网盘和 WebDAV 源
      </p>
      <p class="note-text">
        网盘（WebDisk）和 WebDAV 来源目前后端尚未实现。
        当前支持本地文件夹、网易云音乐 API 与酷狗音乐 API 三个来源。
      </p>
    </div>
  </div>
</template>

<style scoped>
/* ── Unsupported note ──────────────────────────────────────────────── */
.unsupported-note {
  background: var(--bg-secondary, #fafafa);
  border: 1px solid var(--border-color, #e8e8ed);
}

.note-content {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.note-title {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary, #333);
}

.note-icon {
  width: 18px;
  height: 18px;
  color: var(--text-tertiary, #999);
  flex-shrink: 0;
}

.note-text {
  margin: 0;
  font-size: 13px;
  color: var(--text-secondary, #666);
  line-height: 1.5;
}

@media (prefers-color-scheme: dark) {
  .unsupported-note {
    background: var(--bg-secondary, #2c2c2e);
  }
}
</style>