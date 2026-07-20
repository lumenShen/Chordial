<script setup>
/**
 * BatchDownloadManager - 批量下载任务浮窗
 *
 * 监听全局下载进度事件，显示进行中/已完成的批量下载任务。
 * - 默认收起为右下角小图标（带活动数量徽标）
 * - 点击展开为完整面板，列出所有任务及详细进度
 * - 任务完成后 5s 自动从列表移除（downloadManager store 内部处理）
 * - 支持单个任务取消（进行中的任务）和关闭（已完成的任务）
 */
import { ref, computed } from 'vue';
import { useDownloadManager } from '@/stores/downloadManager.js';

const { taskList, activeCount, totalProgress, cancelTask, dismissTask } =
  useDownloadManager();

// 是否展开面板
const expanded = ref(false);
// 取消中的任务ID（避免重复点击）
const cancellingIds = ref(new Set());

const hasTasks = computed(() => taskList.value.length > 0);

/** 单个任务进度百分比 0-100（skipped 也算已处理） */
function taskProgress(task) {
  if (!task.total) return 0;
  const done = (task.succeeded || 0) + (task.failed || 0) + (task.skipped || 0);
  return Math.round((done / task.total) * 100);
}

/** 任务状态文字 */
function taskStatusText(task) {
  const skipped = task.skipped || 0;
  switch (task.status) {
    case 'running':
      return `下载中 ${task.succeeded + task.failed + skipped}/${task.total}`;
    case 'completed':
      return skipped > 0
        ? `完成 ${task.succeeded} 首 · 跳过 ${skipped}`
        : `完成 ${task.succeeded} 首`;
    case 'completed_with_errors':
      return `完成 ${task.succeeded} 成功 / ${task.failed} 失败${skipped > 0 ? ` · 跳过 ${skipped}` : ''}`;
    case 'cancelled':
      return `已取消（${task.succeeded} 已下载${skipped > 0 ? ` · ${skipped} 跳过` : ''}）`;
    default:
      return task.status;
  }
}

/** 任务状态图标类 */
function taskStatusIcon(task) {
  switch (task.status) {
    case 'running':
      return 'bi-arrow-down-circle';
    case 'completed':
      return 'bi-check-circle-fill';
    case 'completed_with_errors':
      return 'bi-exclamation-circle-fill';
    case 'cancelled':
      return 'bi-x-circle-fill';
    default:
      return 'bi-circle';
  }
}

async function onCancel(taskId) {
  if (cancellingIds.value.has(taskId)) return;
  cancellingIds.value.add(taskId);
  try {
    await cancelTask(taskId);
  } catch (e) {
    console.warn('[BatchDownloadManager] 取消任务失败:', e);
  } finally {
    cancellingIds.value.delete(taskId);
  }
}

function onDismiss(taskId) {
  dismissTask(taskId);
}

function togglePanel() {
  expanded.value = !expanded.value;
}
</script>

<template>
  <div v-if="hasTasks" class="bdm-root" :class="{ expanded }">
    <!-- 收起态：右下角小圆按钮 -->
    <button
      v-if="!expanded"
      class="bdm-fab"
      :class="{ active: activeCount > 0 }"
      @click="togglePanel"
      :title="`批量下载（${activeCount} 个进行中）`"
    >
      <i class="bi bi-cloud-arrow-down-fill"></i>
      <span v-if="activeCount > 0" class="bdm-fab-badge">{{ activeCount }}</span>
    </button>

    <!-- 展开态：完整面板 -->
    <transition name="bdm-panel">
      <div v-if="expanded" class="bdm-panel">
        <header class="bdm-header">
          <div class="bdm-title">
            <i class="bi bi-cloud-arrow-down"></i>
            <span>批量下载</span>
            <span v-if="activeCount > 0" class="bdm-title-badge">{{ activeCount }}</span>
          </div>
          <button class="bdm-close" @click="togglePanel" title="收起">
            <i class="bi bi-chevron-down"></i>
          </button>
        </header>

        <div v-if="taskList.length > 1" class="bdm-summary">
          总计 {{ totalProgress.total }} 首 ·
          成功 {{ totalProgress.succeeded }} ·
          跳过 {{ totalProgress.skipped }} ·
          失败 {{ totalProgress.failed }}
        </div>

        <ul class="bdm-task-list">
          <li v-for="task in taskList" :key="task.taskId" class="bdm-task">
            <div class="bdm-task-head">
              <i :class="['bi', taskStatusIcon(task), 'bdm-task-icon', `status-${task.status}`]"></i>
              <div class="bdm-task-info">
                <div class="bdm-task-meta">
                  <span class="bdm-task-status">{{ taskStatusText(task) }}</span>
                  <span v-if="task.status === 'running' && task.currentTitle" class="bdm-task-current">
                    · {{ task.currentTitle }} - {{ task.currentArtist }}
                  </span>
                </div>
                <div class="bdm-task-progress">
                  <div
                    class="bdm-task-progress-bar"
                    :class="{
                      'has-failed': task.failed > 0,
                      'is-cancelled': task.status === 'cancelled',
                    }"
                    :style="{ width: taskProgress(task) + '%' }"
                  ></div>
                </div>
              </div>
              <div class="bdm-task-actions">
                <button
                  v-if="task.status === 'running'"
                  class="bdm-task-btn cancel"
                  :disabled="cancellingIds.has(task.taskId)"
                  @click="onCancel(task.taskId)"
                  title="取消下载"
                >
                  <i class="bi bi-x-lg"></i>
                </button>
                <button
                  v-else
                  class="bdm-task-btn dismiss"
                  @click="onDismiss(task.taskId)"
                  title="移除"
                >
                  <i class="bi bi-x-lg"></i>
                </button>
              </div>
            </div>
          </li>
        </ul>
      </div>
    </transition>
  </div>
</template>

<style scoped>
.bdm-root {
  position: fixed;
  right: 20px;
  bottom: calc(var(--player-bar-height, 100px) + 20px);
  z-index: 200;
  pointer-events: none;
}

.bdm-root > * {
  pointer-events: auto;
}

/* ── FAB 收起态 ─────────────────────────────────── */
.bdm-fab {
  position: relative;
  width: 48px;
  height: 48px;
  border-radius: 50%;
  border: none;
  background: var(--bg-secondary);
  color: var(--text-primary);
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.15);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 20px;
  transition: transform 0.2s, box-shadow 0.2s;
}

.bdm-fab:hover {
  transform: translateY(-2px);
  box-shadow: 0 6px 20px rgba(0, 0, 0, 0.2);
}

.bdm-fab.active {
  background: var(--primary-color);
  color: #fff;
}

.bdm-fab-badge {
  position: absolute;
  top: -4px;
  right: -4px;
  min-width: 18px;
  height: 18px;
  padding: 0 5px;
  border-radius: 9px;
  background: #ff3b30;
  color: #fff;
  font-size: 11px;
  font-weight: 600;
  line-height: 18px;
  text-align: center;
  border: 2px solid var(--bg-secondary);
  box-sizing: content-box;
}

/* ── 展开面板 ─────────────────────────────────── */
.bdm-panel {
  width: min(92vw, 380px);
  max-height: 60vh;
  display: flex;
  flex-direction: column;
  background: var(--bg-secondary);
  border-radius: 14px;
  box-shadow: 0 12px 36px rgba(0, 0, 0, 0.22);
  overflow: hidden;
}

.bdm-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 14px 10px;
  border-bottom: 1px solid var(--border-light);
}

.bdm-title {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
}

.bdm-title > i {
  font-size: 16px;
  color: var(--primary-color);
}

.bdm-title-badge {
  min-width: 18px;
  height: 18px;
  padding: 0 5px;
  border-radius: 9px;
  background: var(--primary-color);
  color: #fff;
  font-size: 11px;
  font-weight: 600;
  line-height: 18px;
  text-align: center;
}

.bdm-close {
  width: 28px;
  height: 28px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  border-radius: 6px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 14px;
  transition: background 0.15s;
}

.bdm-close:hover {
  background: var(--bg-hover);
}

.bdm-summary {
  padding: 6px 14px;
  font-size: 12px;
  color: var(--text-secondary);
  background: var(--bg-tertiary);
}

.bdm-task-list {
  list-style: none;
  margin: 0;
  padding: 6px;
  overflow-y: auto;
  flex: 1;
}

.bdm-task {
  padding: 8px;
  border-radius: 10px;
  transition: background 0.15s;
}

.bdm-task:hover {
  background: var(--bg-hover);
}

.bdm-task-head {
  display: flex;
  align-items: flex-start;
  gap: 10px;
}

.bdm-task-icon {
  font-size: 16px;
  margin-top: 2px;
  flex-shrink: 0;
}

.bdm-task-icon.status-running {
  color: var(--primary-color);
}

.bdm-task-icon.status-completed {
  color: #34c759;
}

.bdm-task-icon.status-completed_with_errors {
  color: #ff9500;
}

.bdm-task-icon.status-cancelled {
  color: var(--text-tertiary);
}

.bdm-task-info {
  flex: 1;
  min-width: 0;
}

.bdm-task-meta {
  font-size: 13px;
  color: var(--text-primary);
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  align-items: baseline;
}

.bdm-task-status {
  font-weight: 500;
}

.bdm-task-current {
  color: var(--text-secondary);
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 100%;
}

.bdm-task-progress {
  margin-top: 6px;
  height: 4px;
  border-radius: 2px;
  background: var(--bg-tertiary);
  overflow: hidden;
}

.bdm-task-progress-bar {
  height: 100%;
  background: var(--primary-color);
  border-radius: 2px;
  transition: width 0.3s ease;
}

.bdm-task-progress-bar.has-failed {
  background: linear-gradient(90deg, var(--primary-color) 0%, var(--primary-color) var(--succeed-pct, 70%), #ff3b30 var(--succeed-pct, 70%), #ff3b30 100%);
}

.bdm-task-progress-bar.is-cancelled {
  background: var(--text-tertiary);
}

.bdm-task-actions {
  flex-shrink: 0;
}

.bdm-task-btn {
  width: 28px;
  height: 28px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  border-radius: 6px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 12px;
  transition: background 0.15s, color 0.15s;
}

.bdm-task-btn:hover {
  background: var(--bg-active);
  color: var(--text-primary);
}

.bdm-task-btn.cancel:hover {
  color: #ff3b30;
}

.bdm-task-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* ── 过渡动画 ─────────────────────────────────── */
.bdm-panel-enter-active,
.bdm-panel-leave-active {
  transition: opacity 0.2s, transform 0.2s;
}

.bdm-panel-enter-from,
.bdm-panel-leave-to {
  opacity: 0;
  transform: translateY(10px) scale(0.95);
}

/* ── 响应式 ─────────────────────────────────── */
@media (max-width: 767px) {
  .bdm-root {
    right: 12px;
    bottom: calc(var(--player-bar-height, 100px) + 12px);
  }

  .bdm-panel {
    width: calc(100vw - 24px);
    max-height: 50vh;
  }
}
</style>
