/**
 * DownloadManagerStore - 批量下载任务全局状态管理
 *
 * 监听后端 `netease-download-progress` 事件，维护所有进行中/已完成的批量下载任务。
 * UI 组件（BatchDownloadManager.vue）通过 useDownloadManager() 订阅状态。
 *
 * 单个任务的生命周期：
 * 1. startBatch(songs) → 调用 netease.downloadSongsBatch → 返回 { task_id, total }
 * 2. tasks[task_id] 创建为 active 状态
 * 3. 收到 progress 事件 → 更新 succeeded/failed/index/currentTrack
 * 4. status=cancelled → 任务被取消（剩余曲目未下载）
 * 5. index+1 >= total → 任务结束（status=completed/failed 由最终统计决定）
 * 6. 任务结束后 5s 自动从 UI 移除（保留 history 供错误查看）
 */
import { reactive, readonly, computed } from 'vue';
import { listen } from '@tauri-apps/api/event';
import { platformIsTauri } from '@/composables/usePlatform.js';
import {
  downloadSongsBatch,
  cancelDownloadTask,
  DOWNLOAD_PROGRESS_EVENT,
} from '@/api/musicSource/netease.js';

// ── 状态 ────────────────────────────────────────────────────
const state = reactive({
  /** @type {Record<string, DownloadTask>} task_id → 任务 */
  tasks: {},
  /** 是否监听已启动（避免重复 listen） */
  started: false,
});

let unlistenFn = null;

// ── 内部辅助 ────────────────────────────────────────────────

/**
 * 处理单条进度事件。
 * @param {import('@/api/musicSource/netease.js').DownloadProgress} p
 */
function handleProgress(p) {
  if (!p?.task_id) return;
  const task = state.tasks[p.task_id];
  if (!task) {
    // 收到未注册任务的进度事件（可能页面刷新后旧任务仍在后台运行）
    // 创建一个临时任务条目以显示进度
    state.tasks[p.task_id] = {
      taskId: p.task_id,
      total: p.total,
      succeeded: p.succeeded,
      failed: p.failed,
      index: p.index,
      status: p.status === 'cancelled' ? 'cancelled' : 'running',
      currentTitle: p.title,
      currentArtist: p.artist,
      lastError: p.error,
      startedAt: Date.now(),
      finishedAt: null,
      items: [
        {
          index: p.index,
          title: p.title,
          artist: p.artist,
          status: p.status,
          path: p.path,
          error: p.error,
        },
      ],
    };
    return;
  }

  task.succeeded = p.succeeded;
  task.failed = p.failed;
  task.index = p.index;
  task.currentTitle = p.title;
  task.currentArtist = p.artist;
  if (p.error) task.lastError = p.error;

  // 累积单曲结果
  task.items[p.index] = {
    index: p.index,
    title: p.title,
    artist: p.artist,
    status: p.status,
    path: p.path,
    error: p.error,
  };

  // 状态转换
  if (p.status === 'cancelled') {
    task.status = 'cancelled';
    task.finishedAt = Date.now();
    scheduleRemoval(p.task_id);
  } else if (p.index + 1 >= p.total) {
    // 最后一首完成
    task.status = task.failed > 0 ? 'completed_with_errors' : 'completed';
    task.finishedAt = Date.now();
    scheduleRemoval(p.task_id);
  }
}

/** 任务结束后延迟 5s 自动从 UI 移除（保留足够时间让用户查看结果） */
function scheduleRemoval(taskId) {
  setTimeout(() => {
    if (state.tasks[taskId]?.finishedAt) {
      delete state.tasks[taskId];
    }
  }, 5000);
}

// ── 公开 API ────────────────────────────────────────────────

/**
 * 启动一个批量下载任务。
 * @param {import('@/api/musicSource/netease.js').BatchDownloadItem[]} songs
 * @returns {Promise<{ taskId: string, total: number }>}
 */
async function startBatch(songs) {
  if (!songs?.length) {
    throw new Error('待下载列表为空');
  }
  const result = await downloadSongsBatch(songs);
  const taskId = result.task_id;
  state.tasks[taskId] = {
    taskId,
    total: result.total,
    succeeded: 0,
    failed: 0,
    index: -1,
    status: 'running',
    currentTitle: '',
    currentArtist: '',
    lastError: null,
    startedAt: Date.now(),
    finishedAt: null,
    /** @type {Array<{index:number,title:string,artist:string,status:string,path:string|null,error:string|null}>} */
    items: new Array(result.total).fill(null),
  };
  return { taskId, total: result.total };
}

/**
 * 取消指定任务。
 * @param {string} taskId
 */
async function cancelTask(taskId) {
  return cancelDownloadTask(taskId);
}

/**
 * 手动从 UI 移除任务（用于已结束任务的关闭按钮）。
 * @param {string} taskId
 */
function dismissTask(taskId) {
  delete state.tasks[taskId];
}

/** 启动事件监听（幂等） */
async function start() {
  if (state.started) return;
  if (!platformIsTauri()) return;
  state.started = true;
  try {
    unlistenFn = await listen(DOWNLOAD_PROGRESS_EVENT, (e) => handleProgress(e.payload));
  } catch (e) {
    console.error('[downloadManager] 监听下载进度事件失败:', e);
    state.started = false;
  }
}

/** 停止监听（应用卸载时调用） */
function stop() {
  if (unlistenFn) {
    unlistenFn();
    unlistenFn = null;
  }
  state.started = false;
}

// ── 计算属性 ────────────────────────────────────────────────
const taskList = computed(() => Object.values(state.tasks));
const activeCount = computed(() =>
  taskList.value.filter((t) => t.status === 'running').length,
);
const totalProgress = computed(() => {
  const list = taskList.value;
  if (!list.length) return { succeeded: 0, failed: 0, total: 0 };
  return list.reduce(
    (acc, t) => ({
      succeeded: acc.succeeded + t.succeeded,
      failed: acc.failed + t.failed,
      total: acc.total + t.total,
    }),
    { succeeded: 0, failed: 0, total: 0 },
  );
});

export function useDownloadManager() {
  // 首次调用时启动监听
  if (!state.started) start();
  return {
    state: readonly(state),
    taskList,
    activeCount,
    totalProgress,
    startBatch,
    cancelTask,
    dismissTask,
  };
}

export function shutdownDownloadManager() {
  stop();
}
