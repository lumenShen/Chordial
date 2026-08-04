<script setup>
/**
 * PlaylistStatus — 搜索页的状态区
 *
 * 按优先级渲染：错误 → 加载中 → 空态（未搜索 / 无结果）。
 * 当有结果时渲染 nothing（由父组件以 v-if/v-else 切换结果列表）。
 */
defineProps({
  errorMsg: { type: String, default: '' },
  isLoading: { type: Boolean, default: false },
  query: { type: String, default: '' },
  hasResults: { type: Boolean, default: false },
});
</script>

<template>
  <!-- 错误 -->
  <div v-if="errorMsg" class="error-state">
    <HIcon name="exclamation-triangle" />
    <span>{{ errorMsg }}</span>
  </div>

  <!-- 加载 -->
  <div v-else-if="isLoading" class="loading-state">
    <div class="spinner"></div>
    <p>搜索中…</p>
  </div>

  <!-- 空状态：未搜索 -->
  <div v-else-if="!query && !hasResults" class="empty-state">
    <HIcon name="cloud-search" class="empty-icon" />
    <h3 class="empty-title">输入关键词开始搜索</h3>
    <p class="empty-desc">需要先在「设置 → 网易云音乐来源」配置 API 服务器</p>
    <router-link to="/settings/netease" class="btn btn-primary">去配置</router-link>
  </div>

  <!-- 空状态：无结果 -->
  <div v-else-if="!hasResults" class="empty-state">
    <HIcon name="emoji-frown" class="empty-icon" />
    <h3 class="empty-title">未找到 "{{ query }}" 的相关歌单</h3>
    <p class="empty-desc">试试其他关键词</p>
  </div>
</template>

<style scoped>
.error-state,
.loading-state,
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 60px 20px;
  text-align: center;
  color: var(--text-secondary, #666);
  gap: 8px;
}

.empty-icon {
  font-size: 56px;
  color: var(--text-tertiary, #999);
  margin-bottom: 8px;
  opacity: 0.5;
}

.empty-title {
  font-size: 18px;
  font-weight: 700;
  color: var(--text-primary, #333);
  margin: 0;
}

.empty-desc {
  font-size: 13px;
  margin: 0 0 16px;
}

.btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 8px 16px;
  font-size: 13px;
  font-weight: 500;
  border: 1px solid var(--border-color, #e8e8e8);
  border-radius: 8px;
  background: var(--bg-primary, #fff);
  color: var(--text-primary, #333);
  cursor: pointer;
  text-decoration: none;
  transition: all 0.2s ease;
}

.btn:hover:not(:disabled) {
  border-color: var(--primary-color);
  color: var(--primary-color);
}

.btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.btn-primary {
  background: var(--primary-color);
  border-color: var(--primary-color);
  color: #fff;
}

.btn-primary:hover:not(:disabled) {
  background: var(--primary-hover);
  color: #fff;
}

.spinner {
  width: 32px;
  height: 32px;
  border: 3px solid var(--border-light, #e8e8e8);
  border-top-color: var(--primary-color);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}
</style>