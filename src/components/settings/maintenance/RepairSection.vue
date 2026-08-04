<template>
  <!-- 数据修复 -->
  <div class="settings-section repair-section">
    <h3 class="block-title">
      <HIcon name="wrench-adjustable" />
      数据修复
    </h3>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">修复歌手/专辑引用</label>
        <span class="setting-desc">
          修复 song.artist_ids / song.album_id 中引用的无效 UUID。适用场景：点击歌手详情页时报"歌手不存在 / 找不到该歌手的信息"。修复后无需重新扫描即可正常访问。
        </span>
      </div>
      <div class="setting-control">
        <button
          class="btn btn-primary"
          :class="{ confirming: pendingAction === 'repair_references' }"
          :disabled="busy !== null"
          @click="$emit('confirm-action', 'repair_references')"
        >
          {{ pendingAction === 'repair_references' ? '确认修复' : '修复' }}
        </button>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">音乐去重（保留最高音质）</label>
        <span class="setting-desc">
          按 (标题 + 艺术家) 分组所有本地文件，每组只保留音质最高的版本，删除其他重复文件。
          Web 来源（如网易云）记录不受影响。删除前请确保已停止播放。耗时取决于库大小。
        </span>
      </div>
      <div class="setting-control">
        <button
          class="btn btn-primary"
          :class="{ confirming: pendingAction === 'dedup_keep_highest' }"
          :disabled="busy !== null"
          @click="$emit('confirm-action', 'dedup_keep_highest')"
        >
          {{ pendingAction === 'dedup_keep_highest' ? '确认去重' : '去重' }}
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

.btn-primary {
  background: var(--primary-light);
  color: var(--primary-color);
  border-color: var(--primary-color);
}

.btn-primary:hover:not(:disabled) {
  background: var(--primary-light);
}

.btn-primary.confirming {
  background: var(--primary-color);
  color: #fff;
  border-color: var(--primary-color);
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