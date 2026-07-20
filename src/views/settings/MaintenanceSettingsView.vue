<template>
  <div ref="rootRef" class="maintenance-view">
    <div class="section-header">
      <h2 class="section-title">数据维护</h2>
      <p class="section-subtitle">查看应用数据占用并执行清理操作</p>
    </div>

    <!-- 库统计 -->
    <div class="settings-section">
      <h3 class="block-title">
        <i class="bi bi-bar-chart"></i>
        库统计
        <button
          class="refresh-btn"
          @click="loadStats"
          :disabled="loading"
          title="刷新"
        >
          <i class="bi bi-arrow-clockwise" :class="{ spinning: loading }"></i>
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
        <i class="bi bi-folder2-open"></i>
        数据文件位置
      </h3>
      <div class="path-item" v-for="(path, key) in stats.paths" :key="key">
        <span class="path-label">{{ pathLabels[key] || key }}</span>
        <code class="path-value" :title="path">{{ path }}</code>
      </div>
    </div>

    <!-- 数据修复 -->
    <div class="settings-section repair-section">
      <h3 class="block-title">
        <i class="bi bi-wrench-adjustable"></i>
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
            @click="confirmAction('repair_references')"
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
            @click="confirmAction('dedup_keep_highest')"
          >
            {{ pendingAction === 'dedup_keep_highest' ? '确认去重' : '去重' }}
          </button>
        </div>
      </div>
    </div>

    <!-- 清理操作 -->
    <div class="settings-section danger-section">
      <h3 class="block-title danger">
        <i class="bi bi-exclamation-triangle"></i>
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
            @click="confirmAction('clear_library')"
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
            @click="confirmAction('clear_cache')"
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
            @click="confirmAction('clear_storage')"
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
            @click="confirmAction('reset_all')"
          >
            {{ pendingAction === 'reset_all' ? '确认重置' : '重置' }}
          </button>
        </div>
      </div>
    </div>

    <!-- Toast -->
    <transition name="fade">
      <div v-if="toast" class="toast" :class="toast.type">
        <i class="bi" :class="toastIcon"></i>
        <span>{{ toast.message }}</span>
      </div>
    </transition>
  </div>
</template>

<script setup>
import { ref, computed, onMounted, onUnmounted, useTemplateRef } from 'vue';
import { useAnime } from '@/composables/useAnime.js';
import {
  maintenanceStats,
  maintenanceClearLibrary,
  maintenanceClearCache,
  maintenanceClearStorage,
  maintenanceResetAll,
  maintenanceRepairReferences,
  libraryDedupKeepHighestQuality,
} from '@/api/tauri';

const rootRef = useTemplateRef('root');
const { run } = useAnime(() => rootRef.value);

const stats = ref(null);
const loading = ref(false);
const busy = ref(null); // 当前正在执行的操作名（null 表示空闲）
const pendingAction = ref(null); // 等待二次确认的操作名
let confirmTimer = null;

const toast = ref(null);
let toastTimer = null;

const pathLabels = computed(() => ({
  library: '音乐库',
  storage: '通用存储',
  config: '应用配置',
  folder_store: '文件夹列表',
}));

const toastIcon = computed(() => {
  if (!toast.value) return '';
  return {
    success: 'bi-check-circle-fill',
    error: 'bi-x-circle-fill',
    info: 'bi-info-circle-fill',
  }[toast.value.type] || '';
});

function formatBytes(bytes) {
  if (!bytes || bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  const v = bytes / Math.pow(1024, i);
  return `${v.toFixed(v >= 100 || i === 0 ? 0 : 1)} ${units[i]}`;
}

function showToast(message, type = 'info', duration = 3000) {
  toast.value = { message, type };
  if (toastTimer) clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    toast.value = null;
  }, duration);
}

async function loadStats() {
  loading.value = true;
  try {
    stats.value = await maintenanceStats();
  } catch (e) {
    showToast(`加载统计失败：${e.message || e}`, 'error');
  } finally {
    loading.value = false;
  }
}

function confirmAction(action) {
  // 已在执行中，忽略
  if (busy.value) return;

  // 已在等待确认，且点的是同一个操作 → 执行
  if (pendingAction.value === action) {
    if (confirmTimer) {
      clearTimeout(confirmTimer);
      confirmTimer = null;
    }
    pendingAction.value = null;
    executeAction(action);
    return;
  }

  // 进入二次确认状态，3 秒内未再次点击则取消
  pendingAction.value = action;
  if (confirmTimer) clearTimeout(confirmTimer);
  confirmTimer = setTimeout(() => {
    pendingAction.value = null;
  }, 3000);
}

async function executeAction(action) {
  busy.value = action;
  try {
    let result;
    switch (action) {
      case 'clear_library':
        result = await maintenanceClearLibrary();
        showToast(result.message || '音乐库已清空', 'success');
        break;
      case 'clear_cache':
        result = await maintenanceClearCache();
        showToast(`缓存已清空（内存 ${result.memory_items} 项 / Blob ${result.blob_items} 项）`, 'success');
        break;
      case 'clear_storage':
        result = await maintenanceClearStorage();
        showToast(`通用存储已清空（${result.items} 项）`, 'success');
        break;
      case 'reset_all':
        result = await maintenanceResetAll();
        showToast(result.message || '所有数据已清空', 'success', 5000);
        break;
      case 'repair_references':
        result = await maintenanceRepairReferences();
        showToast(result.message || '引用已修复', 'success', 5000);
        break;
      case 'dedup_keep_highest':
        result = await libraryDedupKeepHighestQuality();
        showToast(
          `去重完成：处理 ${result.groups_processed} 组，删除 ${result.removed_count} 个重复文件`,
          'success',
          5000,
        );
        break;
    }
    // 操作完成后刷新统计
    await loadStats();
  } catch (e) {
    showToast(`操作失败：${e.message || e}`, 'error', 5000);
  } finally {
    busy.value = null;
  }
}

onMounted(() => {
  loadStats();
  run(({ animate, stagger, presets }) => {
    animate('.section-header', { ...presets.fadeIn });
    animate('.settings-section', { ...presets.fadeInUp, delay: stagger(80) });
  });
});

onUnmounted(() => {
  if (confirmTimer) clearTimeout(confirmTimer);
  if (toastTimer) clearTimeout(toastTimer);
});
</script>

<style scoped>
.maintenance-view {
  max-width: 720px;
  margin: 0 auto;
  position: relative;
  padding-bottom: 80px;
}

.section-header {
  margin-bottom: 24px;
}

.section-title {
  font-size: 24px;
  font-weight: 800;
  margin: 0 0 6px;
  color: var(--text-primary, #333);
}

.section-subtitle {
  font-size: 13px;
  color: var(--text-secondary, #666);
  margin: 0;
}

.settings-section {
  background: var(--bg-secondary, #fff);
  border-radius: 16px;
  padding: 20px 24px;
  margin-bottom: 16px;
  border: 1px solid var(--border-light, #e8e8e8);
}

.danger-section {
  border-color: rgba(220, 53, 69, 0.2);
}

.block-title {
  font-size: 15px;
  font-weight: 700;
  margin: 0 0 16px;
  color: var(--text-primary, #333);
  display: flex;
  align-items: center;
  gap: 8px;
}

.block-title i {
  color: var(--primary-color, #0078d7);
}

.block-title.danger i {
  color: #dc3545;
}

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
  color: var(--primary-color, #0078d7);
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

.setting-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 0;
  border-bottom: 1px solid var(--border-light, #e8e8e8);
  gap: 16px;
}

.setting-item:last-child {
  border-bottom: none;
}

.setting-info {
  flex: 1;
  min-width: 0;
}

.setting-label {
  display: block;
  font-size: 14px;
  font-weight: 500;
  color: var(--text-primary, #333);
  margin-bottom: 3px;
}

.setting-label.danger-text {
  color: #dc3545;
}

.setting-desc {
  display: block;
  font-size: 12px;
  color: var(--text-secondary, #666);
  line-height: 1.5;
}

.setting-control {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-shrink: 0;
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

.btn:disabled {
  cursor: not-allowed;
  opacity: 0.5;
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

.btn-primary {
  background: rgba(0, 120, 215, 0.12);
  color: #0078d7;
  border-color: rgba(0, 120, 215, 0.3);
}

.btn-primary:hover:not(:disabled) {
  background: rgba(0, 120, 215, 0.2);
}

.btn-primary.confirming {
  background: #0078d7;
  color: #fff;
  border-color: #0078d7;
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

.toast {
  position: fixed;
  bottom: 32px;
  left: 50%;
  transform: translateX(-50%);
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px 20px;
  border-radius: 12px;
  font-size: 14px;
  font-weight: 500;
  color: #fff;
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.15);
  z-index: 1000;
  pointer-events: none;
  max-width: 90vw;
}

.toast.success {
  background: #28a745;
}

.toast.error {
  background: #dc3545;
}

.toast.info {
  background: #0078d7;
}

.toast i {
  font-size: 16px;
}

.fade-enter-active,
.fade-leave-active {
  transition: all 0.3s ease;
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
  transform: translateX(-50%) translateY(12px);
}

@media (max-width: 767px) {
  .stats-grid {
    grid-template-columns: repeat(2, 1fr);
  }

  .setting-item {
    flex-direction: column;
    align-items: flex-start;
  }

  .setting-control {
    width: 100%;
    justify-content: flex-end;
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
