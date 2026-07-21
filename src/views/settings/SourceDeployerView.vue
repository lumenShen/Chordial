<template>
  <div ref="rootRef" class="deployer-view">
    <div class="section-header">
      <h2 class="section-title">音乐源服务部署</h2>
      <p class="section-subtitle">
        一键部署网易云 / 酷狗 / QQ 音乐 API 服务到本地，并设置程序启动时自动运行
      </p>
    </div>

    <!-- 工具链检测 -->
    <div class="settings-section">
      <h3 class="block-title">
        <i class="bi bi-tools"></i>
        工具链检测
        <button class="refresh-btn" @click="loadToolchain" :disabled="toolchainLoading" title="重新检测">
          <i class="bi bi-arrow-clockwise" :class="{ spinning: toolchainLoading }"></i>
        </button>
      </h3>

      <div v-if="!toolchain" class="stats-loading">检测中…</div>
      <div v-else class="toolchain-grid">
        <div class="toolchain-item" :class="{ ok: toolchain.git_available, bad: !toolchain.git_available }">
          <i class="bi" :class="toolchain.git_available ? 'bi-check-circle-fill' : 'bi-x-circle-fill'"></i>
          <div class="toolchain-info">
            <div class="toolchain-name">git</div>
            <div class="toolchain-version">{{ toolchain.git_version || '未检测到' }}</div>
          </div>
        </div>
        <div class="toolchain-item" :class="{ ok: toolchain.node_available, bad: !toolchain.node_available }">
          <i class="bi" :class="toolchain.node_available ? 'bi-check-circle-fill' : 'bi-x-circle-fill'"></i>
          <div class="toolchain-info">
            <div class="toolchain-name">node</div>
            <div class="toolchain-version">{{ toolchain.node_version || '未检测到' }}</div>
          </div>
        </div>
        <div class="toolchain-item" :class="{ ok: toolchain.npm_available, bad: !toolchain.npm_available }">
          <i class="bi" :class="toolchain.npm_available ? 'bi-check-circle-fill' : 'bi-x-circle-fill'"></i>
          <div class="toolchain-info">
            <div class="toolchain-name">npm</div>
            <div class="toolchain-version">{{ toolchain.npm_version || '未检测到' }}</div>
          </div>
        </div>
      </div>

      <div v-if="toolchain && !allToolsAvailable" class="toolchain-warn">
        <i class="bi bi-exclamation-triangle-fill"></i>
        缺少必要工具，请先安装 <a href="https://git-scm.com/downloads" target="_blank" rel="noopener">Git</a> 与
        <a href="https://nodejs.org/" target="_blank" rel="noopener">Node.js</a>（含 npm）后重启应用。
      </div>
    </div>

    <!-- 来源列表 -->
    <div class="settings-section" v-for="src in sources" :key="src.spec.id">
      <h3 class="block-title">
        <i class="bi" :class="sourceIcon(src.spec.id)"></i>
        {{ src.spec.display_name }}
        <span class="status-badge" :class="statusClass(src.runtime.status)">
          {{ statusLabel(src.runtime.status) }}
        </span>
      </h3>

      <p class="source-desc">{{ src.spec.description }}</p>
      <p class="source-meta">
        <span class="meta-item"><i class="bi bi-github"></i> <a :href="src.spec.git_url" target="_blank" rel="noopener">{{ src.spec.git_url }}</a></span>
        <span class="meta-item"><i class="bi bi-hdd-network"></i> 端口 {{ effectivePort(src) }}</span>
        <span v-if="src.runtime.pid" class="meta-item"><i class="bi bi-cpu"></i> PID {{ src.runtime.pid }}</span>
      </p>

      <div v-if="src.config.last_error" class="error-banner">
        <i class="bi bi-exclamation-triangle-fill"></i>
        <pre class="error-text">{{ src.config.last_error }}</pre>
      </div>

      <!-- 配置 -->
      <div class="setting-item">
        <div class="setting-info">
          <label class="setting-label">监听端口</label>
          <span class="setting-desc">0 = 使用默认端口（{{ src.spec.default_port }}）</span>
        </div>
        <div class="setting-control">
          <input
            type="number"
            min="0"
            max="65535"
            :value="src.config.port"
            @change="onPortChange(src, $event)"
            class="number-input"
            :disabled="src.runtime.status === 'running'"
          />
        </div>
      </div>

      <div class="setting-item">
        <div class="setting-info">
          <label class="setting-label">启用快速部署</label>
          <span class="setting-desc">关闭后忽略「自动运行」标志</span>
        </div>
        <div class="setting-control">
          <label class="toggle">
            <input
              type="checkbox"
              :checked="src.config.enabled"
              @change="onToggleEnabled(src, $event)"
            />
            <span class="toggle-slider"></span>
          </label>
        </div>
      </div>

      <div class="setting-item">
        <div class="setting-info">
          <label class="setting-label">程序启动时自动运行</label>
          <span class="setting-desc">应用启动时自动拉起此服务</span>
        </div>
        <div class="setting-control">
          <label class="toggle">
            <input
              type="checkbox"
              :checked="src.config.auto_start"
              :disabled="!src.config.enabled"
              @change="onToggleAutoStart(src, $event)"
            />
            <span class="toggle-slider"></span>
          </label>
        </div>
      </div>

      <div class="setting-item">
        <div class="setting-info">
          <label class="setting-label">启动后自动配置 base_url</label>
          <span class="setting-desc">把对应来源（{{ src.spec.id }}）的 base_url 设为 {{ src.runtime.base_url }}</span>
        </div>
        <div class="setting-control">
          <label class="toggle">
            <input
              type="checkbox"
              :checked="src.config.auto_configure_base_url"
              @change="onToggleAutoBaseUrl(src, $event)"
            />
            <span class="toggle-slider"></span>
          </label>
        </div>
      </div>

      <!-- 操作按钮 -->
      <div class="action-row">
        <button
          v-if="src.runtime.status === 'not_deployed'"
          class="btn btn-primary"
          :disabled="busy[src.spec.id] || !allToolsAvailable"
          @click="deployAndStart(src)"
        >
          <i class="bi bi" :class="busy[src.spec.id] ? 'bi-hourglass-split' : 'bi-rocket-takeoff'"></i>
          {{ busy[src.spec.id] ? busy[src.spec.id] : '一键部署并启动' }}
        </button>

        <template v-else>
          <button
            v-if="src.runtime.status !== 'running'"
            class="btn btn-primary"
            :disabled="busy[src.spec.id] || !allToolsAvailable"
            @click="startSource(src)"
          >
            <i class="bi bi-play-fill"></i> 启动
          </button>
          <button
            v-else
            class="btn btn-secondary"
            :disabled="busy[src.spec.id]"
            @click="stopSource(src)"
          >
            <i class="bi bi-stop-fill"></i> 停止
          </button>

          <button
            class="btn"
            :disabled="busy[src.spec.id] || src.runtime.status === 'running'"
            @click="pullSource(src)"
            title="拉取最新代码（git pull）"
          >
            <i class="bi bi-arrow-repeat"></i> 更新代码
          </button>

          <button
            class="btn"
            :disabled="busy[src.spec.id] || src.runtime.status === 'running'"
            @click="installSource(src)"
            title="重新安装依赖（npm install）"
          >
            <i class="bi bi-box-seam"></i> 重装依赖
          </button>

          <button
            class="btn btn-danger-ghost"
            :disabled="busy[src.spec.id]"
            @click="confirmRemove(src)"
          >
            <i class="bi bi-trash"></i> {{ pendingRemove === src.spec.id ? '确认删除' : '移除部署' }}
          </button>
        </template>
      </div>

      <!-- 日志展开 -->
      <div class="logs-section" v-if="src.runtime.recent_logs && src.runtime.recent_logs.length > 0">
        <div class="logs-header" @click="toggleLogs(src.spec.id)">
          <i class="bi bi" :class="expandedLogs[src.spec.id] ? 'bi-chevron-down' : 'bi-chevron-right'"></i>
          <span>最近日志（{{ src.runtime.recent_logs.length }} 条）</span>
        </div>
        <pre v-if="expandedLogs[src.spec.id]" class="logs-content">{{ src.runtime.recent_logs.join('\n') }}</pre>
      </div>
    </div>

    <!-- Toast -->
    <transition name="toast">
      <div v-if="toast" class="toast" :class="toast.type">
        <i class="bi" :class="toastIcon"></i>
        <span>{{ toast.message }}</span>
      </div>
    </transition>
  </div>
</template>

<script setup>
import { ref, computed, onMounted, onUnmounted, useTemplateRef, reactive } from 'vue';
import { useAnime } from '@/composables/useAnime.js';
import * as deployerApi from '@/api/musicSource/deployer.js';
import * as neteaseApi from '@/api/musicSource/netease.js';
import * as kugouApi from '@/api/musicSource/kugou.js';
import * as qqApi from '@/api/musicSource/qq.js';

const rootRef = useTemplateRef('root');
const { run } = useAnime(() => rootRef.value);

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
  run(({ animate, presets }) => {
    animate('.section-header', { ...presets.fadeIn });
  });
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
    } else if (src.spec.id === 'qq') {
      const cfg = await qqApi.getConfig();
      cfg.base_url = baseUrl;
      cfg.enabled = true;
      await qqApi.updateConfig(cfg);
    }
    showToast(`${src.spec.display_name} 的 base_url 已自动配置为 ${baseUrl}`, 'success');
  } catch (e) {
    console.warn('autoConfigureBaseUrl failed:', e);
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
    qq: 'bi-music-player',
  }[id] || 'bi-cloud';
}

function showToast(message, type = 'info', duration = 3000) {
  toast.value = { message, type };
  if (toastTimer) clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    toast.value = null;
  }, duration);
}
</script>

<style scoped>
.deployer-view {
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

/* 工具链 */
.toolchain-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 12px;
  margin-bottom: 12px;
}

.toolchain-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 12px;
  border-radius: 10px;
  background: var(--bg-tertiary, rgba(0, 0, 0, 0.03));
  border: 1px solid var(--border-light, #e8e8e8);
}

.toolchain-item.ok {
  border-color: rgba(40, 167, 69, 0.3);
}

.toolchain-item.bad {
  border-color: rgba(220, 53, 69, 0.3);
}

.toolchain-item i {
  font-size: 18px;
}

.toolchain-item.ok i {
  color: #28a745;
}

.toolchain-item.bad i {
  color: #dc3545;
}

.toolchain-info {
  min-width: 0;
}

.toolchain-name {
  font-weight: 600;
  font-size: 13px;
  color: var(--text-primary, #333);
}

.toolchain-version {
  font-size: 12px;
  color: var(--text-secondary, #666);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.toolchain-warn {
  margin-top: 12px;
  padding: 10px 12px;
  border-radius: 8px;
  background: rgba(255, 193, 7, 0.12);
  color: #856404;
  font-size: 13px;
  display: flex;
  align-items: center;
  gap: 8px;
}

.toolchain-warn a {
  color: #856404;
  text-decoration: underline;
}

/* 来源卡片 */
.source-desc {
  font-size: 13px;
  color: var(--text-secondary, #666);
  margin: 0 0 8px;
}

.source-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 16px;
  margin: 0 0 16px;
  font-size: 12px;
  color: var(--text-secondary, #666);
}

.source-meta a {
  color: var(--primary-color, #0078d7);
  text-decoration: none;
}

.source-meta a:hover {
  text-decoration: underline;
}

.meta-item {
  display: inline-flex;
  align-items: center;
  gap: 4px;
}

.status-badge {
  margin-left: auto;
  padding: 2px 10px;
  border-radius: 12px;
  font-size: 12px;
  font-weight: 600;
}

.status-badge.ok {
  background: rgba(40, 167, 69, 0.15);
  color: #28a745;
}

.status-badge.bad {
  background: rgba(220, 53, 69, 0.15);
  color: #dc3545;
}

.status-badge.info {
  background: rgba(0, 120, 215, 0.12);
  color: var(--primary-color, #0078d7);
}

.status-badge.neutral {
  background: rgba(0, 0, 0, 0.06);
  color: var(--text-secondary, #666);
}

.error-banner {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 10px 12px;
  border-radius: 8px;
  background: rgba(220, 53, 69, 0.1);
  color: #dc3545;
  font-size: 13px;
  margin-bottom: 12px;
}

.error-banner > i {
  flex-shrink: 0;
  margin-top: 2px;
}

.error-text {
  margin: 0;
  font-family: 'Consolas', 'Monaco', monospace;
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-all;
  flex: 1;
  min-width: 0;
}

/* 设置项 */
.setting-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 0;
  border-bottom: 1px solid var(--border-lightest, rgba(0, 0, 0, 0.04));
}

.setting-item:last-of-type {
  border-bottom: none;
}

.setting-info {
  flex: 1;
  min-width: 0;
  margin-right: 16px;
}

.setting-label {
  display: block;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary, #333);
  margin-bottom: 2px;
}

.setting-desc {
  display: block;
  font-size: 12px;
  color: var(--text-secondary, #666);
  line-height: 1.5;
}

.setting-control {
  flex-shrink: 0;
}

.number-input {
  width: 100px;
  padding: 6px 10px;
  border: 1px solid var(--border-light, #e8e8e8);
  border-radius: 8px;
  font-size: 13px;
  background: var(--bg-primary, #fff);
  color: var(--text-primary, #333);
}

.number-input:disabled {
  background: var(--bg-tertiary, rgba(0, 0, 0, 0.04));
  cursor: not-allowed;
}

/* Toggle */
.toggle {
  position: relative;
  display: inline-block;
  width: 44px;
  height: 24px;
}

.toggle input {
  opacity: 0;
  width: 0;
  height: 0;
}

.toggle-slider {
  position: absolute;
  cursor: pointer;
  inset: 0;
  background: var(--bg-tertiary, #ccc);
  border-radius: 24px;
  transition: 0.2s;
}

.toggle-slider::before {
  content: '';
  position: absolute;
  width: 18px;
  height: 18px;
  left: 3px;
  top: 3px;
  background: white;
  border-radius: 50%;
  transition: 0.2s;
}

.toggle input:checked + .toggle-slider {
  background: var(--primary-color, #0078d7);
}

.toggle input:checked + .toggle-slider::before {
  transform: translateX(20px);
}

.toggle input:disabled + .toggle-slider {
  opacity: 0.4;
  cursor: not-allowed;
}

/* 操作按钮 */
.action-row {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 16px;
}

.btn {
  padding: 7px 14px;
  border: 1px solid var(--border-light, #e8e8e8);
  border-radius: 8px;
  background: var(--bg-primary, #fff);
  color: var(--text-primary, #333);
  font-size: 13px;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  transition: all 0.15s ease;
}

.btn:hover:not(:disabled) {
  border-color: var(--primary-color, #0078d7);
  color: var(--primary-color, #0078d7);
}

.btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.btn-primary {
  background: var(--primary-color, #0078d7);
  border-color: var(--primary-color, #0078d7);
  color: #fff;
}

.btn-primary:hover:not(:disabled) {
  filter: brightness(0.95);
  color: #fff;
}

.btn-secondary {
  background: var(--bg-secondary, #f5f5f5);
  border-color: var(--border-light, #e8e8e8);
}

.btn-danger-ghost {
  color: #dc3545;
  border-color: rgba(220, 53, 69, 0.3);
}

.btn-danger-ghost:hover:not(:disabled) {
  background: rgba(220, 53, 69, 0.08);
  color: #dc3545;
  border-color: #dc3545;
}

/* 日志 */
.logs-section {
  margin-top: 16px;
  border-top: 1px solid var(--border-lightest, rgba(0, 0, 0, 0.04));
  padding-top: 12px;
}

.logs-header {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-secondary, #666);
  cursor: pointer;
  user-select: none;
}

.logs-header:hover {
  color: var(--primary-color, #0078d7);
}

.logs-content {
  margin: 8px 0 0;
  padding: 12px;
  background: var(--bg-tertiary, rgba(0, 0, 0, 0.04));
  border-radius: 8px;
  font-family: 'Consolas', 'Monaco', monospace;
  font-size: 11px;
  line-height: 1.5;
  color: var(--text-secondary, #666);
  max-height: 300px;
  overflow-y: auto;
  white-space: pre-wrap;
  word-break: break-all;
}

/* Toast */
.toast {
  position: fixed;
  bottom: 32px;
  left: 50%;
  transform: translateX(-50%);
  padding: 12px 20px;
  border-radius: 10px;
  background: var(--bg-secondary, #fff);
  border: 1px solid var(--border-light, #e8e8e8);
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.12);
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 13px;
  z-index: 1000;
  max-width: 480px;
}

.toast.success {
  border-color: rgba(40, 167, 69, 0.3);
  color: #28a745;
}

.toast.error {
  border-color: rgba(220, 53, 69, 0.3);
  color: #dc3545;
}

.toast.info {
  color: var(--text-primary, #333);
}

.toast-enter-active,
.toast-leave-active {
  transition: all 0.25s ease;
}

.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translateX(-50%) translateY(8px);
}
</style>
