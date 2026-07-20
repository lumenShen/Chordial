<template>
  <div ref="rootRef" class="qq-view">
    <div class="section-header">
      <h2 class="section-title">QQ 音乐来源</h2>
      <p class="section-subtitle">配置 QQMusicApi 服务器以搜索/下载在线音乐</p>
    </div>

    <!-- 来源配置 -->
    <div class="settings-section">
      <h3 class="block-title">
        <i class="bi bi-cloud-arrow-down"></i>
        API 服务器
      </h3>

      <div class="setting-item">
        <div class="setting-info">
          <label class="setting-label">启用此来源</label>
          <span class="setting-desc">关闭后不注册到来源系统，所有 QQ 命令将返回错误</span>
        </div>
        <div class="setting-control">
          <label class="toggle">
            <input type="checkbox" v-model="config.enabled" />
            <span class="toggle-slider"></span>
          </label>
        </div>
      </div>

      <div class="setting-item column">
        <div class="setting-info">
          <label class="setting-label">API 基础 URL</label>
          <span class="setting-desc">
            QQMusicApi 服务器地址（不含尾随斜杠）。
            <a href="https://github.com/jsososo/QQMusicApi" target="_blank" rel="noopener">项目地址</a>
          </span>
        </div>
        <input
          type="text"
          v-model="config.base_url"
          placeholder="https://qq-api.example.com"
          class="text-input"
        />
      </div>

      <div class="setting-item column">
        <div class="setting-info">
          <label class="setting-label">Cookie（可选）</label>
          <span class="setting-desc">
            含 uin / qqmusic_key 等字段，登录后可访问 VIP 歌曲。
            手动从浏览器或 App 抓包复制，以分号分隔。
          </span>
        </div>
        <div class="login-actions">
          <button class="action-btn ghost" @click="showManualCookie = !showManualCookie">
            <i class="bi bi" :class="showManualCookie ? 'bi-chevron-up' : 'bi-chevron-down'"></i>
            {{ showManualCookie ? '收起' : '展开' }} Cookie 输入
          </button>
        </div>
        <textarea
          v-if="showManualCookie"
          v-model="config.cookie"
          placeholder="uin=123456789; qqmusic_key=xxxxx; ..."
          class="textarea-input"
          rows="3"
        ></textarea>
      </div>

      <div class="setting-item">
        <div class="setting-info">
          <label class="setting-label">请求超时（秒）</label>
          <span class="setting-desc">0 = 默认 15 秒</span>
        </div>
        <div class="setting-control">
          <input
            type="number"
            min="0"
            max="120"
            v-model.number="config.timeout_secs"
            class="number-input"
          />
        </div>
      </div>

      <div class="setting-item column">
        <div class="setting-info">
          <label class="setting-label">下载目录</label>
          <span class="setting-desc">从 QQ 音乐下载的歌曲保存到此目录（空则使用应用默认下载目录）</span>
        </div>
        <div class="path-row">
          <input
            type="text"
            v-model="config.download_dir"
            placeholder="（留空使用默认目录）"
            class="text-input"
          />
          <button class="action-btn" @click="pickDownloadDir">
            <i class="bi bi-folder"></i> 选择
          </button>
        </div>
      </div>

      <div class="setting-actions">
        <button class="action-btn primary" @click="saveConfig" :disabled="saving">
          <i class="bi bi-save"></i> {{ saving ? '保存中…' : '保存配置' }}
        </button>
        <button class="action-btn" @click="testConnection" :disabled="testing">
          <i class="bi bi-wifi" :class="{ spinning: testing }"></i>
          {{ testing ? '测试中…' : '测试连接' }}
        </button>
      </div>

      <div v-if="testResult" class="test-result" :class="{ ok: testResult.ok, fail: !testResult.ok }">
        <i class="bi" :class="testResult.ok ? 'bi-check-circle-fill' : 'bi-x-circle-fill'"></i>
        <span v-if="testResult.ok">连接成功（{{ testResult.base_url }}）</span>
        <span v-else>连接失败：{{ testResult.error || '未知错误' }}</span>
      </div>
    </div>

    <!-- 使用说明 -->
    <div class="settings-section">
      <h3 class="block-title">
        <i class="bi bi-info-circle"></i>
        使用说明
      </h3>
      <div class="info-block">
        <p><strong>音质等级</strong>：QQ 音乐支持以下音质（下载时可通过音质选择器切换）：</p>
        <ul>
          <li><code>128</code> — 标准（128 kbps MP3）</li>
          <li><code>m4a</code> — 较高（AAC，通常 &lt; 128 kbps）</li>
          <li><code>320</code> — 极高（320 kbps MP3，默认）</li>
          <li><code>ape</code> — 无损（APE）</li>
          <li><code>flac</code> — 无损（FLAC 16bit/44.1kHz）</li>
        </ul>
        <p><strong>entity_id 格式</strong>：QQ 歌曲使用 <code>song:&lt;songmid&gt;</code> 作为唯一标识（字符串 mid）。</p>
        <p><strong>歌词</strong>：QQ 音乐歌词通过 songmid 拉取，支持 LRC 格式（含翻译）。</p>
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
import { ref, computed, onMounted, useTemplateRef } from 'vue';
import { useAnime } from '@/composables/useAnime.js';
import * as qqApi from '@/api/musicSource/qq.js';

const rootRef = useTemplateRef('root');
const { run } = useAnime(() => rootRef.value);

// ── 配置状态 ────────────────────────────────────────────────
const config = ref({
  base_url: '',
  cookie: '',
  timeout_secs: 0,
  enabled: false,
  download_dir: '',
});

const showManualCookie = ref(false);

// ── Toast ───────────────────────────────────────────────────
const toast = ref(null);
let toastTimer = null;

const toastIcon = computed(() => {
  if (!toast.value) return '';
  return {
    success: 'bi-check-circle-fill',
    error: 'bi-x-circle-fill',
    info: 'bi-info-circle-fill',
  }[toast.value.type] || '';
});

function showToast(message, type = 'info', duration = 3000) {
  toast.value = { message, type };
  if (toastTimer) clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    toast.value = null;
  }, duration);
}

// ── 配置加载/保存 ───────────────────────────────────────────
async function loadConfig() {
  try {
    config.value = await qqApi.getConfig();
  } catch (e) {
    showToast(`加载配置失败：${e.message || e}`, 'error');
  }
}

const saving = ref(false);
async function saveConfig() {
  saving.value = true;
  try {
    await qqApi.updateConfig(config.value);
    showToast('配置已保存', 'success');
  } catch (e) {
    showToast(`保存失败：${e.message || e}`, 'error');
  } finally {
    saving.value = false;
  }
}

// ── 连接测试 ────────────────────────────────────────────────
const testing = ref(false);
const testResult = ref(null);

async function testConnection() {
  if (!config.value.base_url) {
    showToast('请先填写 API 基础 URL', 'error');
    return;
  }
  testing.value = true;
  testResult.value = null;
  try {
    // 先保存配置（确保后端使用最新 URL）
    await qqApi.updateConfig(config.value);
    const result = await qqApi.testConnection();
    testResult.value = result;
    if (result.ok) {
      showToast('连接测试成功', 'success');
    } else {
      showToast(`连接失败：${result.error || '未知错误'}`, 'error');
    }
  } catch (e) {
    testResult.value = { ok: false, error: e.message || String(e), base_url: config.value.base_url };
    showToast(`连接测试异常：${e.message || e}`, 'error');
  } finally {
    testing.value = false;
  }
}

// ── 选择下载目录 ────────────────────────────────────────────
async function pickDownloadDir() {
  try {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const selected = await open({ directory: true, multiple: false });
    if (selected) {
      config.value.download_dir = selected;
    }
  } catch (e) {
    showToast('无法打开目录选择器，请手动输入路径', 'info');
  }
}

// ── 初始化 ──────────────────────────────────────────────────
onMounted(async () => {
  await loadConfig();
  run(({ animate, stagger, presets }) => {
    animate('.section-header', { ...presets.fadeIn });
    animate('.settings-section', { ...presets.fadeInUp, delay: stagger(80) });
  });
});
</script>

<style scoped>
.qq-view {
  max-width: 720px;
  margin: 0 auto;
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

.setting-item.column {
  flex-direction: column;
  align-items: stretch;
  gap: 10px;
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

.setting-desc {
  display: block;
  font-size: 12px;
  color: var(--text-secondary, #666);
  line-height: 1.5;
}

.setting-desc a {
  color: var(--primary-color, #0078d7);
  text-decoration: none;
}

.setting-desc a:hover {
  text-decoration: underline;
}

.setting-control {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-shrink: 0;
}

.text-input,
.textarea-input,
.number-input {
  width: 100%;
  padding: 8px 12px;
  font-size: 13px;
  border: 1px solid var(--border-color, #e8e8e8);
  border-radius: 8px;
  background: var(--bg-primary, #fff);
  color: var(--text-primary, #333);
  outline: none;
  font-family: inherit;
  box-sizing: border-box;
}

.textarea-input {
  resize: vertical;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 12px;
}

.number-input {
  width: 100px;
  text-align: right;
}

.path-row {
  display: flex;
  gap: 8px;
  align-items: center;
}

.path-row .text-input {
  flex: 1;
}

.toggle {
  position: relative;
  display: inline-block;
  width: 44px;
  height: 22px;
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
  background-color: var(--border-color, #ccc);
  transition: 0.3s;
  border-radius: 22px;
}

.toggle-slider:before {
  position: absolute;
  content: "";
  height: 16px;
  width: 16px;
  left: 3px;
  bottom: 3px;
  background-color: white;
  transition: 0.3s;
  border-radius: 50%;
}

.toggle input:checked + .toggle-slider {
  background-color: var(--primary-color, #0078d7);
}

.toggle input:checked + .toggle-slider:before {
  transform: translateX(22px);
}

.setting-actions {
  display: flex;
  gap: 12px;
  margin-top: 16px;
  padding-top: 16px;
  border-top: 1px solid var(--border-light, #e8e8e8);
}

.action-btn {
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
  transition: all 0.2s ease;
}

.action-btn:hover:not(:disabled) {
  border-color: var(--primary-color, #0078d7);
  color: var(--primary-color, #0078d7);
}

.action-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.action-btn.primary {
  background: var(--primary-color, #0078d7);
  border-color: var(--primary-color, #0078d7);
  color: #fff;
}

.action-btn.primary:hover:not(:disabled) {
  background: var(--primary-color-dark, #0066bf);
  color: #fff;
}

.action-btn.ghost {
  background: transparent;
  border-color: var(--border-color, #e8e8e8);
  color: var(--text-secondary, #666);
  font-size: 12px;
  padding: 6px 12px;
}

.test-result {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 12px;
  padding: 10px 14px;
  border-radius: 8px;
  font-size: 13px;
}

.test-result.ok {
  background: rgba(40, 167, 69, 0.1);
  color: #28a745;
}

.test-result.fail {
  background: rgba(220, 53, 69, 0.1);
  color: #dc3545;
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
  background: var(--bg-secondary, #fff);
  border: 1px solid var(--border-light, #e8e8e8);
  border-radius: 12px;
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.12);
  font-size: 13px;
  z-index: 1000;
  max-width: 90vw;
}

.toast.success {
  color: #28a745;
  border-color: #28a745;
}

.toast.error {
  color: #dc3545;
  border-color: #dc3545;
}

.toast.info {
  color: var(--primary-color, #0078d7);
}

.spinning {
  animation: spin 1s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.2s, transform 0.2s;
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
  transform: translate(-50%, 8px);
}

/* ── 使用说明 ──────────────────────────────────────────────── */
.info-block {
  font-size: 13px;
  color: var(--text-secondary, #666);
  line-height: 1.7;
}

.info-block p {
  margin: 0 0 8px;
}

.info-block ul {
  margin: 0 0 12px;
  padding-left: 20px;
}

.info-block li {
  margin-bottom: 4px;
}

.info-block code {
  padding: 1px 6px;
  background: var(--bg-tertiary, rgba(15, 15, 15, 0.05));
  border-radius: 4px;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 12px;
  color: var(--text-primary, #333);
}

.login-actions {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}

@media (max-width: 767px) {
  .setting-item {
    flex-direction: column;
    align-items: flex-start;
    gap: 10px;
  }

  .setting-control {
    width: 100%;
    justify-content: space-between;
  }

  .setting-actions {
    flex-direction: column;
  }

  .action-btn {
    width: 100%;
    justify-content: center;
  }
}
</style>
