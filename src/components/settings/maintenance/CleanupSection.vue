<template>
  <!-- 清理操作 -->
  <div class="settings-section danger-section">
    <h3 class="block-title danger">
      <HIcon name="exclamation-triangle" />
      清理操作
    </h3>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">清空音乐库</label>
        <span class="setting-desc">删除所有歌曲/艺人/专辑/歌词及本地索引与文件夹列表。保留配置、存储与缓存。</span>
      </div>
      <div class="setting-control">
        <button
          class="btn btn-warn"
          :class="{ confirming: pendingAction === 'clear_library' }"
          :disabled="busy !== null"
          @click="$emit('confirm-action', 'clear_library')"
        >
          {{ pendingAction === 'clear_library' ? '确认清空' : '清空' }}
        </button>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">清空缓存</label>
        <span class="setting-desc">清空内存缓存与 Blob 磁盘缓存（如专辑封面）。不影响音乐库。</span>
      </div>
      <div class="setting-control">
        <button
          class="btn btn-warn"
          :class="{ confirming: pendingAction === 'clear_cache' }"
          :disabled="busy !== null"
          @click="$emit('confirm-action', 'clear_cache')"
        >
          {{ pendingAction === 'clear_cache' ? '确认清空' : '清空' }}
        </button>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">清空通用存储</label>
        <span class="setting-desc">清空 storage.json 中所有键值（歌词文本、播放列表等）。不影响音乐库与配置。</span>
      </div>
      <div class="setting-control">
        <button
          class="btn btn-warn"
          :class="{ confirming: pendingAction === 'clear_storage' }"
          :disabled="busy !== null"
          @click="$emit('confirm-action', 'clear_storage')"
        >
          {{ pendingAction === 'clear_storage' ? '确认清空' : '清空' }}
        </button>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label danger-text">重置全部数据</label>
        <span class="setting-desc">恢复出厂设置：清空音乐库、本地索引、文件夹、配置、存储与缓存。建议重启应用。</span>
      </div>
      <div class="setting-control">
        <button
          class="btn btn-danger"
          :class="{ confirming: pendingAction === 'reset_all' }"
          :disabled="busy !== null"
          @click="$emit('confirm-action', 'reset_all')"
        >
          {{ pendingAction === 'reset_all' ? '确认重置' : '重置' }}
        </button>
      </div>
    </div>
  </div>
</template>

<script setup>
defineProps({
  pendingAction: { type: String, default: null },
  busy: { type: String, default: null },
});

defineEmits(['confirm-action']);
</script>

<style scoped>
.danger-section {
  border-color: rgba(220, 53, 69, 0.2);
}

.block-title.danger i {
  color: #dc3545;
}

.setting-label.danger-text {
  color: #dc3545;
}

.btn {
  padding: 8px 16px;
  font-size: 13px;
  font-weight: 500;
  border-radius: 8px;
  border: 1px solid transparent;
  cursor: pointer;
  transition: all 0.2s ease;
  white-space: nowrap;
  min-width: 72px;
}

.btn-warn {
  background: rgba(255, 193, 7, 0.12);
  color: #b8860b;
  border-color: rgba(255, 193, 7, 0.3);
}

.btn-warn:hover:not(:disabled) {
  background: rgba(255, 193, 7, 0.2);
}

.btn-warn.confirming {
  background: #ffc107;
  color: #000;
  border-color: #ffc107;
  animation: pulse 1s ease-in-out infinite;
}

.btn-danger {
  background: rgba(220, 53, 69, 0.12);
  color: #dc3545;
  border-color: rgba(220, 53, 69, 0.3);
}

.btn-danger:hover:not(:disabled) {
  background: rgba(220, 53, 69, 0.2);
}

.btn-danger.confirming {
  background: #dc3545;
  color: #fff;
  border-color: #dc3545;
  animation: pulse 1s ease-in-out infinite;
}

@keyframes pulse {
  0%, 100% { transform: scale(1); }
  50% { transform: scale(1.05); }
}

@media (max-width: 767px) {
  .setting-control {
    width: 100%;
    justify-content: flex-end;
  }
}
</style>