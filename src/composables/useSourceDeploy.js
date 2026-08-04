import { ref, computed, onMounted, onUnmounted, reactive } from 'vue';
import * as deployerApi from '@/api/musicSource/deployer.js';
import * as neteaseApi from '@/api/musicSource/netease.js';
import * as kugouApi from '@/api/musicSource/kugou.js';

/**
 * 音乐源服务部署合式逻辑
 * 封装：来源列表加载、工具链检测、部署/启动/停止/更新/重装/移除等操作、
 * 配置变更、日志轮询、Toast 提示与二次确认。
 */
export function useSourceDeploy() {
  // ── State ────────────────────────────────────────────────────────────────
  const sources = ref([]);
  const toolchain = ref(null);
  const toolchainLoading = ref(false);
  const busy = reactive({}); // source_id → 当前步骤描述字符串（如 "克隆中…"），无则未在执行
  const expandedLogs = reactive({});
  const pendingRemove = ref(null);
  let removeTimer = null;

  const toast = ref(null);
  let toastTimer = null;

  // ── Computed ──────────────────────────────────────────────────────────────
  const allToolsAvailable = computed(
    () => toolchain.value?.git_available && toolchain.value?.node_available && toolchain.value?.npm_available
  );

  const toastIcon = computed(() => {
    if (!toast.value) return '';
    return {
      success: 'bi-check-circle-fill',
      error: 'bi-x-circle-fill',
      info: 'bi-info-circle-fill',
    }[toast.value.type] || '';
  });

  // ── Lifecycle ─────────────────────────────────────────────────────────────
  onMounted(async () => {
    await Promise.all([loadSources(), loadToolchain()]);
    // 启动日志轮询（运行中的服务实时更新日志）
    startLogPolling();
  });

  onUnmounted(() => {
    stopLogPolling();
    if (removeTimer) clearTimeout(removeTimer);
    if (toastTimer) clearTimeout(toastTimer);
  });

  let logPollHandle = null;
  function startLogPolling() {
    stopLogPolling();
    // 每 3 秒刷新一次（仅在有运行中服务时实际触发网络）
    logPollHandle = setInterval(() => {
      if (sources.value.some((s) => s.runtime.status === 'running')) {
        loadSources();
      }
    }, 3000);
  }
  function stopLogPolling() {
    if (logPollHandle) {
      clearInterval(logPollHandle);
      logPollHandle = null;
    }
  }

  // ── Data loading ──────────────────────────────────────────────────────────
  async function loadSources() {
    try {
      sources.value = await deployerApi.listSources();
    } catch (e) {
      showToast(`加载来源失败：${e.message || e}`, 'error');
    }
  }

  async function loadToolchain() {
    toolchainLoading.value = true;
    try {
      toolchain.value = await deployerApi.checkToolchain();
    } catch (e) {
      showToast(`工具链检测失败：${e.message || e}`, 'error');
    } finally {
      toolchainLoading.value = false;
    }
  }

  // ── Actions ───────────────────────────────────────────────────────────────
  async function deployAndStart(src) {
    const id = src.spec.id;
    try {
      busy[id] = '克隆中…';
      await deployerApi.cloneSource(id);
      busy[id] = '安装依赖中…';
      await deployerApi.installSource(id);
      busy[id] = '启动中…';
      await deployerApi.startSource(id);
      await loadSources();
      showToast(`${src.spec.display_name} 部署并启动成功`, 'success');
      // 自动配置 base_url（若启用）
      if (src.config.auto_configure_base_url) {
        await autoConfigureBaseUrl(src);
      }
    } catch (e) {
      showToast(`${src.spec.display_name} 部署失败：${e.message || e}`, 'error', 6000);
      await loadSources();
    } finally {
      delete busy[id];
    }
  }

  async function startSource(src) {
    const id = src.spec.id;
    try {
      busy[id] = '启动中…';
      await deployerApi.startSource(id);
      await loadSources();
      showToast(`${src.spec.display_name} 已启动`, 'success');
      if (src.config.auto_configure_base_url) {
        await autoConfigureBaseUrl(src);
      }
    } catch (e) {
      showToast(`启动失败：${e.message || e}`, 'error');
    } finally {
      delete busy[id];
    }
  }

  async function stopSource(src) {
    const id = src.spec.id;
    try {
      busy[id] = '停止中…';
      await deployerApi.stopSource(id);
      await loadSources();
      showToast(`${src.spec.display_name} 已停止`, 'success');
    } catch (e) {
      showToast(`停止失败：${e.message || e}`, 'error');
    } finally {
      delete busy[id];
    }
  }

  async function pullSource(src) {
    const id = src.spec.id;
    try {
      busy[id] = '拉取代码中…';
      await deployerApi.pullSource(id);
      showToast(`${src.spec.display_name} 代码已更新`, 'success');
    } catch (e) {
      showToast(`拉取失败：${e.message || e}`, 'error');
    } finally {
      delete busy[id];
    }
  }

  async function installSource(src) {
    const id = src.spec.id;
    try {
      busy[id] = '安装依赖中…';
      await deployerApi.installSource(id);
      showToast(`${src.spec.display_name} 依赖安装完成`, 'success');
      await loadSources();
    } catch (e) {
      showToast(`安装失败：${e.message || e}`, 'error');
      await loadSources();
    } finally {
      delete busy[id];
    }
  }

  function confirmRemove(src) {
    // 二次确认：3 秒内再次点击才真正删除
    if (pendingRemove.value === src.spec.id) {
      if (removeTimer) {
        clearTimeout(removeTimer);
        removeTimer = null;
      }
      pendingRemove.value = null;
      doRemove(src);
      return;
    }
    pendingRemove.value = src.spec.id;
    if (removeTimer) clearTimeout(removeTimer);
    removeTimer = setTimeout(() => {
      pendingRemove.value = null;
    }, 3000);
  }

  async function doRemove(src) {
    const id = src.spec.id;
    try {
      busy[id] = '移除中…';
      await deployerApi.removeSource(id);
      await loadSources();
      showToast(`${src.spec.display_name} 部署已移除`, 'success');
    } catch (e) {
      showToast(`移除失败：${e.message || e}`, 'error');
    } finally {
      delete busy[id];
    }
  }

  // ── 配置变更 ───────────────────────────────────────────────────────────────
  async function onPortChange(src, ev) {
    const port = parseInt(ev.target.value, 10) || 0;
    try {
      const newCfg = { ...src.config, port };
      await deployerApi.updateConfig(src.spec.id, newCfg);
      src.config.port = port;
      showToast(`${src.spec.display_name} 端口已更新为 ${port || src.spec.default_port}`, 'success');
    } catch (e) {
      showToast(`端口更新失败：${e.message || e}`, 'error');
    }
  }

  async function onToggleEnabled(src, ev) {
    const enabled = ev.target.checked;
    try {
      await deployerApi.setEnabled(src.spec.id, enabled);
      src.config.enabled = enabled;
    } catch (e) {
      showToast(`更新失败：${e.message || e}`, 'error');
      ev.target.checked = !enabled;
    }
  }

  async function onToggleAutoStart(src, ev) {
    const autoStart = ev.target.checked;
    try {
      await deployerApi.setAutoStart(src.spec.id, autoStart);
      src.config.auto_start = autoStart;
    } catch (e) {
      showToast(`更新失败：${e.message || e}`, 'error');
      ev.target.checked = !autoStart;
    }
  }

  async function onToggleAutoBaseUrl(src, ev) {
    const v = ev.target.checked;
    try {
      const newCfg = { ...src.config, auto_configure_base_url: v };
      await deployerApi.updateConfig(src.spec.id, newCfg);
      src.config.auto_configure_base_url = v;
    } catch (e) {
      showToast(`更新失败：${e.message || e}`, 'error');
      ev.target.checked = !v;
    }
  }

  // ── 自动配置对应来源的 base_url ────────────────────────────────────────────
  async function autoConfigureBaseUrl(src) {
    try {
      const baseUrl = await deployerApi.getBaseUrl(src.spec.id);
      if (!baseUrl) return;
      if (src.spec.id === 'netease') {
        const cfg = await neteaseApi.getConfig();
        cfg.base_url = baseUrl;
        cfg.enabled = true;
        await neteaseApi.updateConfig(cfg);
      } else if (src.spec.id === 'kugou') {
        const cfg = await kugouApi.getConfig();
        cfg.base_url = baseUrl;
        cfg.enabled = true;
        await kugouApi.updateConfig(cfg);
      }
      showToast(`${src.spec.display_name} 的 base_url 已自动配置为 ${baseUrl}`, 'success');
    } catch (e) {
      // 不再静默吞错：自动配置失败时源会处于"已启用但 base_url 未配置"的不可用状态
      console.warn('autoConfigureBaseUrl failed:', e);
      showToast(`自动配置 base_url 失败：${e.message || e}，请到来源设置页手动填写`, 'error', 6000);
    }
  }

  // ── Helpers ───────────────────────────────────────────────────────────────
  function toggleLogs(id) {
    expandedLogs[id] = !expandedLogs[id];
  }

  function effectivePort(src) {
    return src.config.port > 0 ? src.config.port : src.spec.default_port;
  }

  function statusLabel(status) {
    return {
      not_deployed: '未部署',
      cloned: '已克隆',
      installed: '已安装',
      running: '运行中',
      stopped: '已停止',
      error: '错误',
    }[status] || status;
  }

  function statusClass(status) {
    return {
      not_deployed: 'neutral',
      cloned: 'info',
      installed: 'info',
      running: 'ok',
      stopped: 'neutral',
      error: 'bad',
    }[status] || 'neutral';
  }

  function sourceIcon(id) {
    return {
      netease: 'bi-cloud-arrow-down',
      kugou: 'bi-music-note-beamed',
    }[id] || 'bi-cloud';
  }

  function showToast(message, type = 'info', duration = 3000) {
    toast.value = { message, type };
    if (toastTimer) clearTimeout(toastTimer);
    toastTimer = setTimeout(() => {
      toast.value = null;
    }, duration);
  }

  return {
    sources,
    toolchain,
    toolchainLoading,
    busy,
    expandedLogs,
    pendingRemove,
    toast,
    toastIcon,
    allToolsAvailable,
    loadToolchain,
    deployAndStart,
    startSource,
    stopSource,
    pullSource,
    installSource,
    confirmRemove,
    onPortChange,
    onToggleEnabled,
    onToggleAutoStart,
    onToggleAutoBaseUrl,
    toggleLogs,
    effectivePort,
    statusLabel,
    statusClass,
    sourceIcon,
  };
}