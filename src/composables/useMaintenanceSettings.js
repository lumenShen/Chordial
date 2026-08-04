import { ref, computed, onMounted, onUnmounted } from 'vue';
import {
  maintenanceStats,
  maintenanceClearLibrary,
  maintenanceClearCache,
  maintenanceClearStorage,
  maintenanceResetAll,
  maintenanceRepairReferences,
  libraryDedupKeepHighestQuality,
} from '@/api/tauri';

/**
 * 数据维护页合式逻辑
 * 封装：统计加载、二次确认（3 秒超时）、各类维护/清理操作执行、Toast 提示与生命周期清理。
 * 对外暴露 stats / loading / busy / pendingAction / toast 等状态，以及 loadStats / confirmAction。
 */
export function useMaintenanceSettings() {
  // ── State ────────────────────────────────────────────────────────────────
  const stats = ref(null);
  const loading = ref(false);
  const busy = ref(null); // 当前正在执行的操作名（null 表示空闲）
  const pendingAction = ref(null); // 等待二次确认的操作名
  let confirmTimer = null;

  const toast = ref(null);
  let toastTimer = null;

  // ── Computed ──────────────────────────────────────────────────────────────
  const toastIcon = computed(() => {
    if (!toast.value) return '';
    return {
      success: 'bi-check-circle-fill',
      error: 'bi-x-circle-fill',
      info: 'bi-info-circle-fill',
    }[toast.value.type] || '';
  });

  // ── Helpers ───────────────────────────────────────────────────────────────
  function showToast(message, type = 'info', duration = 3000) {
    toast.value = { message, type };
    if (toastTimer) clearTimeout(toastTimer);
    toastTimer = setTimeout(() => {
      toast.value = null;
    }, duration);
  }

  // ── Data loading ──────────────────────────────────────────────────────────
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

  // ── Actions ───────────────────────────────────────────────────────────────
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

  // ── Lifecycle ─────────────────────────────────────────────────────────────
  onMounted(() => {
    loadStats();
  });

  onUnmounted(() => {
    if (confirmTimer) clearTimeout(confirmTimer);
    if (toastTimer) clearTimeout(toastTimer);
  });

  return {
    stats,
    loading,
    busy,
    pendingAction,
    toast,
    toastIcon,
    loadStats,
    confirmAction,
  };
}