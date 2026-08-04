<script setup>
defineProps({
  isLoading: { type: Boolean, default: false },
  folders: { type: Array, default: () => [] },
  isRemoving: { type: Boolean, default: false },
  getSourceTypeLabel: { type: Function, required: true },
});
defineEmits(['delete-source', 'add-source']);
</script>

<template>
  <div>
    <!-- Loading -->
    <div v-if="isLoading" class="loading-state">
      <div class="spinner"></div>
    </div>

    <!-- Folder list -->
    <template v-else>
      <div v-if="folders.length > 0" class="sources-list">
        <div v-for="folder in folders" :key="folder.path" class="source-card card">
          <div class="source-info">
            <div class="source-icon">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/>
              </svg>
            </div>
            <div class="source-details">
              <h3 class="source-name">{{ folder.name }}</h3>
              <p class="source-path">{{ folder.path }}</p>
              <div class="source-meta">
                <span class="source-type">{{ getSourceTypeLabel() }}</span>
              </div>
            </div>
          </div>
          <div class="source-actions">
            <button
              class="btn-icon btn-delete"
              @click="$emit('delete-source', folder)"
              :disabled="isRemoving"
              title="移除文件夹（不删除磁盘文件）"
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <polyline points="3 6 5 6 21 6"/>
                <path d="M19 6v14a2 2 0 01-2 2H7a2 2 0 01-2-2V6m3 0V4a2 2 0 012-2h4a2 2 0 012 2v2"/>
              </svg>
            </button>
          </div>
        </div>
      </div>

      <!-- Empty state -->
      <div v-else class="empty-state">
        <svg class="empty-state-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
          <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/>
        </svg>
        <h3 class="empty-state-title">暂无本地音乐源</h3>
        <p class="empty-state-desc">添加本地音乐文件夹开始管理你的音乐</p>
        <button class="btn btn-primary" style="margin-top: 16px;" @click="$emit('add-source')">
          添加文件夹
        </button>
      </div>
    </template>
  </div>
</template>

<style scoped>
.sources-list {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.source-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 20px;
}

.source-info {
  display: flex;
  align-items: center;
  gap: 16px;
  min-width: 0;
}

.source-icon {
  width: 48px;
  height: 48px;
  border-radius: var(--radius-md, 8px);
  background: var(--bg-tertiary, rgba(15, 15, 15, 0.05));
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--primary-color);
  flex-shrink: 0;
}

.source-icon svg {
  width: 24px;
  height: 24px;
}

.source-details {
  min-width: 0;
}

.source-name {
  font-size: 16px;
  font-weight: 600;
  color: var(--text-primary, #333);
  margin: 0 0 4px 0;
}

.source-path {
  font-size: 13px;
  color: var(--text-secondary, #666);
  margin: 0 0 8px 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 500px;
}

.source-meta {
  font-size: 12px;
  color: var(--text-tertiary, #999);
}

.source-type {
  display: inline-block;
  padding: 2px 8px;
  background: var(--bg-tertiary, #e8e8ed);
  border-radius: 4px;
  font-weight: 500;
}

.source-actions {
  display: flex;
  gap: 8px;
  flex-shrink: 0;
}

.btn-icon {
  width: 36px;
  height: 36px;
  border: none;
  border-radius: var(--radius-md, 8px);
  background: transparent;
  color: var(--text-secondary, #666);
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  transition: all 0.2s ease;
}

.btn-icon:hover {
  background: var(--hover-bg, rgba(0, 0, 0, 0.05));
  color: var(--text-primary, #333);
}

.btn-icon svg {
  width: 18px;
  height: 18px;
}

.btn-delete:hover {
  background: rgba(239, 68, 68, 0.1);
  color: #ef4444;
}

.btn-icon:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.btn-icon:disabled:hover {
  background: transparent;
  color: var(--text-secondary, #666);
}

/* ── Responsive ────────────────────────────────────────────────────── */
@media (max-width: 767px) {
  .source-card {
    flex-direction: column;
    align-items: flex-start;
    gap: 16px;
  }

  .source-path {
    max-width: 280px;
  }

  .source-actions {
    width: 100%;
    justify-content: flex-end;
  }

  /* 移动端点击区域 ≥ 40px */
  .btn-icon {
    width: 40px;
    height: 40px;
  }
}

@media (prefers-color-scheme: dark) {
  .source-name {
    color: var(--text-primary, #f6f6f6);
  }
}
</style>