<template>
  <!-- 库统计 -->
  <div class="settings-section">
    <h3 class="block-title">
      <HIcon name="bar-chart" />
      库统计
      <button
        class="refresh-btn"
        @click="$emit('refresh')"
        :disabled="loading"
        title="刷新"
      >
        <HIcon name="arrow-clockwise" :class="{ spinning: loading }" />
      </button>
    </h3>

    <div v-if="loading && !stats" class="stats-loading">加载中…</div>
    <template v-else-if="stats">
      <div class="stats-grid">
        <div class="stat-item">
          <div class="stat-value">{{ stats.counts.songs }}</div>
          <div class="stat-label">歌曲</div>
        </div>
        <div class="stat-item">
          <div class="stat-value">{{ stats.counts.artists }}</div>
          <div class="stat-label">艺人</div>
        </div>
        <div class="stat-item">
          <div class="stat-value">{{ stats.counts.albums }}</div>
          <div class="stat-label">专辑</div>
        </div>
        <div class="stat-item">
          <div class="stat-value">{{ stats.counts.lyrics }}</div>
          <div class="stat-label">歌词</div>
        </div>
        <div class="stat-item">
          <div class="stat-value">{{ stats.counts.folders }}</div>
          <div class="stat-label">监听文件夹</div>
        </div>
        <div class="stat-item">
          <div class="stat-value">{{ stats.counts.indexed_files }}</div>
          <div class="stat-label">已索引文件</div>
        </div>
      </div>

      <div class="stat-row">
        <span class="stat-row-label">磁盘占用估算</span>
        <span class="stat-row-value">{{ formatBytes(stats.disk_usage_bytes) }}</span>
      </div>
    </template>
    <div v-else class="stats-loading">暂无数据</div>
  </div>

  <!-- 数据文件位置 -->
  <div class="settings-section" v-if="stats">
    <h3 class="block-title">
      <HIcon name="folder2-open" />
      数据文件位置
    </h3>
    <div class="path-item" v-for="(path, key) in stats.paths" :key="key">
      <span class="path-label">{{ pathLabels[key] || key }}</span>
      <code class="path-value" :title="path">{{ path }}</code>
    </div>
  </div>
</template>

<script setup>
defineProps({
  stats: { type: Object, default: null },
  loading: { type: Boolean, default: false },
});

defineEmits(['refresh']);

const pathLabels = {
  library: '音乐库',
  storage: '通用存储',
  config: '应用配置',
  folder_store: '文件夹列表',
};

function formatBytes(bytes) {
  if (!bytes || bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  const v = bytes / Math.pow(1024, i);
  return `${v.toFixed(v >= 100 || i === 0 ? 0 : 1)} ${units[i]}`;
}
</script>

<style scoped>
.refresh-btn {
  margin-left: auto;
  background: transparent;
  border: none;
  color: var(--text-secondary, #666);
  cursor: pointer;
  padding: 4px 8px;
  border-radius: 6px;
  font-size: 14px;
  transition: all 0.2s ease;
}

.refresh-btn:hover:not(:disabled) {
  background: var(--bg-hover, rgba(0, 0, 0, 0.04));
  color: var(--primary-color);
}

.refresh-btn:disabled {
  cursor: not-allowed;
  opacity: 0.5;
}

.spinning {
  display: inline-block;
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}

.stats-loading {
  padding: 24px 0;
  text-align: center;
  color: var(--text-secondary, #666);
  font-size: 13px;
}

.stats-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 12px;
  margin-bottom: 16px;
}

.stat-item {
  background: var(--bg-primary, #f8f8f8);
  border-radius: 12px;
  padding: 16px 12px;
  text-align: center;
  border: 1px solid var(--border-light, #eee);
}

.stat-value {
  font-size: 24px;
  font-weight: 800;
  color: var(--text-primary, #333);
  line-height: 1.1;
  margin-bottom: 4px;
}

.stat-label {
  font-size: 12px;
  color: var(--text-secondary, #666);
}

.stat-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 0;
  border-top: 1px solid var(--border-light, #e8e8e8);
}

.stat-row-label {
  font-size: 13px;
  color: var(--text-secondary, #666);
}

.stat-row-value {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary, #333);
}

.path-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 0;
  border-bottom: 1px solid var(--border-light, #e8e8e8);
}

.path-item:last-child {
  border-bottom: none;
}

.path-label {
  font-size: 13px;
  color: var(--text-secondary, #666);
  flex-shrink: 0;
  min-width: 100px;
}

.path-value {
  font-size: 12px;
  color: var(--text-primary, #333);
  background: var(--bg-primary, #f8f8f8);
  padding: 6px 10px;
  border-radius: 6px;
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: 'SF Mono', Consolas, Monaco, monospace;
}

@media (max-width: 767px) {
  .stats-grid {
    grid-template-columns: repeat(2, 1fr);
  }

  .path-item {
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
  }

  .path-value {
    width: 100%;
  }
}
</style>