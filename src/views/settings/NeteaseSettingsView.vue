<template>
  <div ref="rootRef" class="netease-view">
    <div class="section-header">
      <h2 class="section-title">网易云音乐来源</h2>
      <p class="section-subtitle">配置 NeteaseCloudMusicApi 服务器以搜索/下载在线音乐</p>
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
          <span class="setting-desc">关闭后不注册到来源系统，所有网易云命令将返回错误</span>
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
            NeteaseCloudMusicApi 服务器地址（不含尾随斜杠）。
            <a href="https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced" target="_blank" rel="noopener">项目地址</a>
          </span>
        </div>
        <input
          type="text"
          v-model="config.base_url"
          placeholder="https://ncm-api.example.com"
          class="text-input"
        />
      </div>

      <div class="setting-item column">
        <div class="setting-info">
          <label class="setting-label">Cookie</label>
          <span class="setting-desc">登录后的 cookie 字符串（用于访问 VIP 歌曲、私人歌单等）</span>
        </div>
        <textarea
          v-model="config.cookie"
          placeholder="MUSIC_U=xxxxx; __csrf=yyyyy; ..."
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
          <span class="setting-desc">从网易云下载的歌曲保存到此目录（空则使用应用默认下载目录）</span>
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

    <!-- 歌词优先级 -->
    <div class="settings-section">
      <h3 class="block-title">
        <i class="bi bi-music-note-list"></i>
        歌词优先级
      </h3>

      <div class="setting-item column">
        <div class="setting-info">
          <label class="setting-label">来源优先级</label>
          <span class="setting-desc">从前到后依次尝试；勾选的来源才会被使用</span>
        </div>
        <div class="source-order">
          <div
            v-for="(src, idx) in sourceOrderItems"
            :key="src.value"
            class="source-item"
          >
            <label class="source-check">
              <input
                type="checkbox"
                v-model="src.enabled"
                @change="onSourceOrderChange"
              />
              <span>{{ src.label }}</span>
            </label>
            <div class="source-rank">
              <button
                class="rank-btn"
                @click="moveSource(idx, -1)"
                :disabled="idx === 0"
                title="上移"
              ><i class="bi bi-chevron-up"></i></button>
              <button
                class="rank-btn"
                @click="moveSource(idx, 1)"
                :disabled="idx === sourceOrderItems.length - 1"
                title="下移"
              ><i class="bi bi-chevron-down"></i></button>
              <span class="rank-num">#{{ idx + 1 }}</span>
            </div>
          </div>
        </div>
      </div>

      <div class="setting-item">
        <div class="setting-info">
          <label class="setting-label">优先逐字歌词</label>
          <span class="setting-desc">同一来源中，逐字（YRC/QRC/TTML）优先于行级 LRC 优先于纯文本</span>
        </div>
        <div class="setting-control">
          <label class="toggle">
            <input type="checkbox" v-model="pref.prefer_word_by_word" @change="savePreference" />
            <span class="toggle-slider"></span>
          </label>
        </div>
      </div>

      <div class="setting-item">
        <div class="setting-info">
          <label class="setting-label">本地有逐字时仍尝试在线获取</label>
          <span class="setting-desc">用于本地 LRC 升级到 YRC；本地已是逐字则不会重复获取</span>
        </div>
        <div class="setting-control">
          <label class="toggle">
            <input
              type="checkbox"
              v-model="pref.fallback_to_online_for_word_by_word"
              @change="savePreference"
            />
            <span class="toggle-slider"></span>
          </label>
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
import { ref, computed, onMounted, useTemplateRef } from 'vue';
import { useAnime } from '@/composables/useAnime.js';
import * as neteaseApi from '@/api/musicSource/netease.js';
import * as prefApi from '@/api/musicSource/lyricPreference.js';

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

const pref = ref({
  source_order: ['local', 'netease'],
  prefer_word_by_word: true,
  fallback_to_online_for_word_by_word: false,
});

// 来源选项（顺序可调整）
const sourceOptions = [
  { value: 'local', label: '本地（音频嵌入 / .lrc 文件）' },
  { value: 'netease', label: '网易云（YRC / LRC）' },
];

// 当前展示的来源顺序（含启用状态）
const sourceOrderItems = ref([]);

function rebuildSourceOrderItems() {
  // 按 pref.source_order 顺序展示，未在其中的来源附加在末尾并标记为禁用
  const ordered = [];
  for (const value of pref.value.source_order) {
    const opt = sourceOptions.find((o) => o.value === value);
    if (opt) ordered.push({ ...opt, enabled: true });
  }
  for (const opt of sourceOptions) {
    if (!ordered.find((o) => o.value === opt.value)) {
      ordered.push({ ...opt, enabled: false });
    }
  }
  sourceOrderItems.value = ordered;
}

function onSourceOrderChange() {
  // 仅保留勾选的来源
  pref.value.source_order = sourceOrderItems.value
    .filter((s) => s.enabled)
    .map((s) => s.value);
  savePreference();
}

function moveSource(idx, delta) {
  const arr = sourceOrderItems.value;
  const newIdx = idx + delta;
  if (newIdx < 0 || newIdx >= arr.length) return;
  [arr[idx], arr[newIdx]] = [arr[newIdx], arr[idx]];
  onSourceOrderChange();
}

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
    config.value = await neteaseApi.getConfig();
  } catch (e) {
    showToast(`加载配置失败：${e.message || e}`, 'error');
  }
}

const saving = ref(false);
async function saveConfig() {
  saving.value = true;
  try {
    await neteaseApi.updateConfig(config.value);
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
    await neteaseApi.updateConfig(config.value);
    const result = await neteaseApi.testConnection();
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

// ── 歌词偏好 ────────────────────────────────────────────────
async function loadPreference() {
  try {
    pref.value = await prefApi.get();
    rebuildSourceOrderItems();
  } catch (e) {
    showToast(`加载歌词偏好失败：${e.message || e}`, 'error');
  }
}

async function savePreference() {
  try {
    await prefApi.set(pref.value);
  } catch (e) {
    showToast(`保存歌词偏好失败：${e.message || e}`, 'error');
  }
}

// ── 选择下载目录 ────────────────────────────────────────────
async function pickDownloadDir() {
  // 使用 Tauri dialog 选择目录
  try {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const selected = await open({ directory: true, multiple: false });
    if (selected) {
      config.value.download_dir = selected;
    }
  } catch (e) {
    // 桌面端对话框不可用时提示手动输入
    showToast('无法打开目录选择器，请手动输入路径', 'info');
  }
}

// ── 初始化 ──────────────────────────────────────────────────
onMounted(async () => {
  await Promise.all([loadConfig(), loadPreference()]);
  run(({ animate, stagger, presets }) => {
    animate('.section-header', { ...presets.fadeIn });
    animate('.settings-section', { ...presets.fadeInUp, delay: stagger(80) });
  });
});
</script>

<style scoped>
.netease-view {
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

.source-order {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.source-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 14px;
  background: var(--bg-hover, rgba(0, 0, 0, 0.02));
  border-radius: 8px;
  border: 1px solid var(--border-light, #e8e8e8);
}

.source-check {
  display: flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
  font-size: 13px;
  color: var(--text-primary, #333);
}

.source-check input {
  margin: 0;
}

.source-rank {
  display: flex;
  align-items: center;
  gap: 6px;
}

.rank-btn {
  width: 24px;
  height: 24px;
  padding: 0;
  border: 1px solid var(--border-color, #e8e8e8);
  background: var(--bg-primary, #fff);
  color: var(--text-secondary, #666);
  border-radius: 4px;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 11px;
}

.rank-btn:hover:not(:disabled) {
  border-color: var(--primary-color, #0078d7);
  color: var(--primary-color, #0078d7);
}

.rank-btn:disabled {
  opacity: 0.3;
  cursor: not-allowed;
}

.rank-num {
  font-size: 11px;
  color: var(--text-tertiary, #999);
  min-width: 24px;
  text-align: right;
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
