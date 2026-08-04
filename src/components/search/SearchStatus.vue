<script setup>
defineProps({
  errorMsg: { type: String, default: '' },
  neteaseWarning: { type: String, default: '' },
  isLoading: { type: Boolean, default: false },
  hasSearched: { type: Boolean, default: false },
  hasAnyResult: { type: Boolean, default: false },
  query: { type: String, default: '' },
});
</script>

<template>
  <!-- 错误状态 -->
  <div v-if="errorMsg" class="error-state">
    <HIcon name="exclamation-triangle" />
    <span>搜索失败：{{ errorMsg }}</span>
  </div>

  <!-- 网易云来源非致命警告（未配置 / 网络失败）—— 独立显示，不阻塞结果展示 -->
  <div v-else-if="!errorMsg && neteaseWarning && !isLoading" class="warning-state">
    <HIcon name="exclamation-circle" />
    <span>网易云音乐搜索不可用：{{ neteaseWarning }}</span>
  </div>

  <!-- 加载状态 -->
  <div v-else-if="!errorMsg && isLoading" class="loading-state">
    <div class="spinner"></div>
    <p class="loading-text">搜索中...</p>
  </div>

  <!-- 空状态：尚未搜索 -->
  <div v-else-if="!errorMsg && !isLoading && !hasSearched" class="empty-state hint-state">
    <HIcon name="search" class="empty-icon" />
    <h3 class="empty-state-title">输入关键词开始搜索</h3>
    <p class="empty-state-desc">支持歌名、歌手名、专辑名的子串匹配（大小写不敏感）</p>
  </div>

  <!-- 空状态：无结果 -->
  <div v-else-if="!errorMsg && !isLoading && hasSearched && !hasAnyResult" class="empty-state">
    <HIcon name="emoji-frown" class="empty-icon" />
    <h3 class="empty-state-title">未找到 "{{ query }}" 的相关结果</h3>
    <p class="empty-state-desc">试试其他关键词，或调整筛选条件</p>
  </div>
</template>

<style scoped>
/* ── 状态 ── */
.loading-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 60px 20px;
  gap: 16px;
}

.loading-state .spinner {
  animation: none; /* 由 anime.js 驱动 */
  width: 32px;
  height: 32px;
  border: 3px solid var(--border-light);
  border-top-color: var(--primary-color);
  border-radius: 50%;
}

.loading-text {
  font-size: 14px;
  color: var(--text-secondary);
  margin: 0;
}

.error-state {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 16px 20px;
  border-radius: 12px;
  background: rgba(255, 99, 71, 0.1);
  color: #ff6347;
  font-size: 14px;
}

.error-state i {
  font-size: 18px;
}

.warning-state {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 18px;
  margin-bottom: 16px;
  border-radius: 12px;
  background: rgba(255, 193, 7, 0.12);
  color: #c98a00;
  font-size: 13px;
}

.warning-state i {
  font-size: 16px;
}

@media (prefers-color-scheme: dark) {
  .warning-state {
    background: rgba(255, 193, 7, 0.08);
    color: #ffcf4d;
  }
}

.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 80px 20px;
  text-align: center;
  gap: 8px;
}

.empty-state.hint-state {
  padding: 100px 20px;
}

.empty-icon {
  font-size: 56px;
  color: var(--text-tertiary);
  margin-bottom: 8px;
  opacity: 0.5;
}

.empty-state-title {
  font-size: 18px;
  font-weight: 700;
  color: var(--text-primary);
  margin: 0;
}

.empty-state-desc {
  font-size: 13px;
  color: var(--text-tertiary);
  margin: 0;
}

/* ── 响应式 ── */
@media (max-width: 767px) {
  .empty-icon {
    font-size: 44px;
  }
}

@media (prefers-color-scheme: dark) {
  .loading-state .spinner {
    border-color: rgba(255, 255, 255, 0.15);
    border-top-color: var(--primary-color);
  }
}
</style>