<template>
  <div ref="rootRef" class="kugou-view">
    <div class="section-header">
      <h2 class="section-title">酷狗音乐来源</h2>
      <p class="section-subtitle">配置 KuGouMusicApi 服务器以搜索/下载在线音乐</p>
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
          <span class="setting-desc">关闭后不注册到来源系统，所有酷狗命令将返回错误</span>
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
            KuGouMusicApi 服务器地址（不含尾随斜杠）。
            <a href="https://github.com/MakcRe/KuGouMusicApi" target="_blank" rel="noopener">项目地址</a>
          </span>
        </div>
        <input
          type="text"
          v-model="config.base_url"
          placeholder="https://kugou-api.example.com"
          class="text-input"
        />
      </div>

      <div class="setting-item column">
        <div class="setting-info">
          <label class="setting-label">登录状态</label>
          <span class="setting-desc">
            扫码登录后可访问 VIP 歌曲。
            <span v-if="loginStatus.loggedIn" class="login-badge ok">
              <i class="bi bi-check-circle-fill"></i> 已登录<span v-if="loginStatus.userid">（用户 {{ loginStatus.userid }}）</span>
            </span>
            <span v-else class="login-badge warn">
              <i class="bi bi-exclamation-circle-fill"></i> 未登录
            </span>
          </span>
        </div>
        <div class="login-actions">
          <button class="action-btn primary" @click="openQrLogin">
            <i class="bi bi-qr-code"></i> 扫码登录
          </button>
          <button v-if="loginStatus.loggedIn" class="action-btn" @click="logout">
            <i class="bi bi-box-arrow-right"></i> 退出登录
          </button>
          <button class="action-btn ghost" @click="showManualCookie = !showManualCookie">
            <i class="bi bi" :class="showManualCookie ? 'bi-chevron-up' : 'bi-chevron-down'"></i>
            手动填入 Cookie
          </button>
        </div>
        <textarea
          v-if="showManualCookie"
          v-model="config.cookie"
          placeholder="dfid=xxxxx; mid=yyyyy; token=zzzzz; userid=12345; ..."
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
          <span class="setting-desc">从酷狗下载的歌曲保存到此目录（空则使用应用默认下载目录）</span>
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
        <p><strong>音质等级</strong>：酷狗支持以下音质（下载时可通过音质选择器切换）：</p>
        <ul>
          <li><code>128</code> — 标准（128 kbps MP3）</li>
          <li><code>320</code> — 极高（320 kbps MP3，默认）</li>
          <li><code>flac</code> — 无损（FLAC 16bit/44.1kHz）</li>
          <li><code>super</code> — Hi-Res（FLAC 24bit/96kHz）</li>
        </ul>
        <p><strong>entity_id 格式</strong>：酷狗歌曲使用 <code>song:&lt;FileHash&gt;</code> 作为唯一标识（32 位十六进制 hash）。</p>
        <p><strong>歌词</strong>：酷狗歌词通过 hash 匹配，支持 LRC 格式（行级时间戳）。</p>
      </div>
    </div>

    <!-- 扫码登录 Modal -->
    <teleport to="body">
      <transition name="fade">
        <div v-if="qrLogin.visible" class="qr-modal-mask" @click.self="closeQrLogin">
          <div class="qr-modal" role="dialog" aria-modal="true">
            <div class="qr-modal-header">
              <h3 class="qr-modal-title">
                <i class="bi bi-qr-code"></i> 酷狗扫码登录
              </h3>
              <button class="qr-modal-close" @click="closeQrLogin" aria-label="关闭">
                <i class="bi bi-x-lg"></i>
              </button>
            </div>

            <div class="qr-modal-body">
              <!-- loading: 正在获取二维码 -->
              <div v-if="qrLogin.phase === 'loading'" class="qr-loading">
                <div class="qr-spinner"></div>
                <p>正在获取二维码…</p>
              </div>

              <!-- 二维码展示 -->
              <div v-else-if="qrLogin.phase === 'showing' || qrLogin.phase === 'polling'" class="qr-showing">
                <div class="qr-image-wrap" :class="{ expired: qrLogin.status === 'expired' }">
                  <img v-if="qrLogin.qrimg" :src="qrLogin.qrimg" alt="酷狗登录二维码" class="qr-image" />
                  <div v-else class="qr-fallback">
                    <p>无法显示二维码图片</p>
                    <p class="qr-url">{{ qrLogin.qrurl }}</p>
                  </div>
                  <!-- 过期遮罩 -->
                  <div v-if="qrLogin.status === 'expired'" class="qr-expired-overlay">
                    <i class="bi bi-clock-history"></i>
                    <p>二维码已过期</p>
                    <button class="action-btn primary" @click="refreshQr">
                      <i class="bi bi-arrow-clockwise"></i> 重新获取
                    </button>
                  </div>
                </div>

                <div class="qr-status">
                  <div v-if="qrLogin.status === 'waiting'" class="status-text waiting">
                    <i class="bi bi-hourglass-split"></i> 请使用酷狗音乐 App 扫码
                  </div>
                  <div v-else-if="qrLogin.status === 'scanned'" class="status-text scanned">
                    <i class="bi bi-phone"></i> 已扫码，请在手机上确认登录
                  </div>
                  <div v-else-if="qrLogin.status === 'authorized'" class="status-text authorized">
                    <i class="bi bi-check-circle-fill"></i> 授权成功
                  </div>
                  <div v-else-if="qrLogin.status === 'expired'" class="status-text expired">
                    <i class="bi bi-clock-history"></i> 二维码已过期，请重新获取
                  </div>
                  <div v-else-if="qrLogin.error" class="status-text error">
                    <i class="bi bi-x-circle-fill"></i> {{ qrLogin.error }}
                  </div>
                </div>

                <!-- "我已扫码"按钮：仅在 waiting 状态显示，乐观切到 scanned 并立即 check -->
                <button
                  v-if="qrLogin.status === 'waiting'"
                  class="action-btn primary qr-scanned-btn"
                  @click="manualScanned"
                >
                  <i class="bi bi-phone"></i> 我已扫码
                </button>
              </div>

              <!-- 成功 -->
              <div v-else-if="qrLogin.phase === 'success'" class="qr-success">
                <i class="bi bi-check-circle-fill success-icon"></i>
                <h4>登录成功</h4>
                <p v-if="qrLogin.userid">用户 ID：{{ qrLogin.userid }}</p>
                <button class="action-btn primary" @click="closeQrLogin">完成</button>
              </div>
            </div>

            <div class="qr-modal-footer">
              <span class="qr-tip">使用酷狗音乐 App 扫描左侧二维码即可登录</span>
            </div>
          </div>
        </div>
      </transition>
    </teleport>

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
import * as kugouApi from '@/api/musicSource/kugou.js';

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
    config.value = await kugouApi.getConfig();
    updateLoginStatus();
  } catch (e) {
    showToast(`加载配置失败：${e.message || e}`, 'error');
  }
}

const saving = ref(false);
async function saveConfig() {
  saving.value = true;
  try {
    await kugouApi.updateConfig(config.value);
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
    await kugouApi.updateConfig(config.value);
    const result = await kugouApi.testConnection();
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

// ── 登录状态 ──────────────────────────────────────────────────
const loginStatus = ref({
  loggedIn: false,
  userid: '',
});

function updateLoginStatus() {
  const cookie = (config.value.cookie || '').trim();
  const match = /(^|;\s*)userid\s*=\s*(\d+)/.exec(cookie);
  loginStatus.value = {
    loggedIn: /(^|;\s*)token\s*=/.test(cookie) && !!match,
    userid: match ? match[2] : '',
  };
}

// ── 扫码登录 Modal ────────────────────────────────────────────
const qrLogin = ref({
  visible: false,
  phase: 'loading',  // loading | showing | polling | success
  status: 'waiting',  // waiting | scanned | authorized | expired | unknown
  key: '',
  qrurl: '',
  qrimg: '',
  userid: '',
  error: '',
});

let pollAbort = false;
let pollingActive = false;

async function openQrLogin() {
  qrLogin.value = {
    visible: true,
    phase: 'loading',
    status: 'waiting',
    key: '',
    qrurl: '',
    qrimg: '',
    userid: '',
    error: '',
  };
  pollAbort = false;
  await fetchQr();
}

async function fetchQr() {
  try {
    const key = await kugouApi.loginQrKey();
    qrLogin.value.key = key;
    const data = await kugouApi.loginQrCreate(key, true);
    qrLogin.value.qrurl = data.url || '';
    qrLogin.value.qrimg = data.base64 || '';
    qrLogin.value.phase = 'showing';
    qrLogin.value.status = 'waiting';
    startPolling();
  } catch (e) {
    qrLogin.value.error = e.message || String(e);
    showToast(`获取二维码失败：${e.message || e}`, 'error');
  }
}

function refreshQr() {
  stopPolling();
  qrLogin.value.error = '';
  qrLogin.value.status = 'waiting';
  qrLogin.value.phase = 'loading';
  qrLogin.value.qrimg = '';
  qrLogin.value.qrurl = '';
  fetchQr();
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function startPolling() {
  if (pollingActive) return;
  pollingActive = true;
  pollingLoop();
}

async function pollingLoop() {
  await sleep(1000);
  while (pollingActive && qrLogin.value.visible && !pollAbort) {
    const key = qrLogin.value.key;
    if (!key) {
      await sleep(500);
      continue;
    }
    try {
      const resp = await kugouApi.loginQrCheck(key);
      if (!pollingActive || !qrLogin.value.visible || pollAbort) break;
      // statusText: 'expired' | 'waiting' | 'scanned' | 'authorized' | 'unknown'
      qrLogin.value.status = resp.statusText || 'unknown';
      if (resp.userid) qrLogin.value.userid = String(resp.userid);
      if (resp.statusText === 'authorized') {
        try {
          await loadConfig();
          updateLoginStatus();
        } catch (e) {
          console.warn('登录成功后刷新配置失败:', e);
        }
        qrLogin.value.phase = 'success';
        showToast('登录成功', 'success');
        break;
      }
      if (resp.statusText === 'expired') {
        break;
      }
      await sleep(1500);
    } catch (e) {
      qrLogin.value.error = e.message || String(e);
      console.warn('[qr poll] check 失败，2s 后重试:', e);
      await sleep(2000);
    }
  }
  pollingActive = false;
}

async function manualScanned() {
  if (qrLogin.value.status !== 'waiting') return;
  qrLogin.value.status = 'scanned';
  const key = qrLogin.value.key;
  if (!key) return;
  try {
    const resp = await kugouApi.loginQrCheck(key);
    if (!qrLogin.value.visible) return;
    qrLogin.value.status = resp.statusText || 'unknown';
    if (resp.userid) qrLogin.value.userid = String(resp.userid);
    if (resp.statusText === 'authorized') {
      try {
        await loadConfig();
        updateLoginStatus();
      } catch (e) {
        console.warn('登录成功后刷新配置失败:', e);
      }
      qrLogin.value.phase = 'success';
      stopPolling();
      showToast('登录成功', 'success');
    }
  } catch (e) {
    qrLogin.value.error = e.message || String(e);
  }
}

function stopPolling() {
  pollingActive = false;
}

function closeQrLogin() {
  pollAbort = true;
  stopPolling();
  qrLogin.value.visible = false;
}

async function logout() {
  if (!confirm('确定退出酷狗登录？清除 Cookie 后将无法访问 VIP 歌曲。')) return;
  // 清除 token 和 userid，保留其他 cookie 字段（如 dfid/mid）
  const existing = (config.value.cookie || '').trim();
  if (existing) {
    const kept = existing
      .split(';')
      .map((s) => s.trim())
      .filter((s) => {
        const k = s.split('=')[0]?.trim();
        return k && k !== 'token' && k !== 'userid';
      })
      .join('; ');
    config.value.cookie = kept;
  } else {
    config.value.cookie = '';
  }
  try {
    await kugouApi.updateConfig(config.value);
    updateLoginStatus();
    showToast('已退出登录', 'info');
  } catch (e) {
    showToast(`退出失败：${e.message || e}`, 'error');
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
.kugou-view {
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

<style>
/* 非 scoped：teleport 到 body 的元素需要全局样式 */
.qr-modal-mask {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.55);
  backdrop-filter: blur(6px);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 2000;
  padding: 16px;
}

.qr-modal {
  width: 100%;
  max-width: 400px;
  background: var(--bg-primary, #fff);
  border-radius: 16px;
  box-shadow: 0 20px 60px rgba(0, 0, 0, 0.3);
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

.qr-modal-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 16px 20px;
  border-bottom: 1px solid var(--border-light, #e8e8e8);
}

.qr-modal-title {
  font-size: 16px;
  font-weight: 700;
  margin: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--text-primary, #333);
}

.qr-modal-title i {
  color: var(--primary-color, #0078d7);
}

.qr-modal-close {
  border: none;
  background: transparent;
  color: var(--text-secondary, #666);
  cursor: pointer;
  padding: 6px;
  border-radius: 6px;
  font-size: 14px;
  transition: background 0.2s;
}

.qr-modal-close:hover {
  background: var(--bg-hover, rgba(0, 0, 0, 0.05));
  color: var(--text-primary, #333);
}

.qr-modal-body {
  padding: 28px 20px;
  min-height: 320px;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-direction: column;
}

.qr-loading {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 16px;
  color: var(--text-secondary, #666);
  font-size: 14px;
}

.qr-spinner {
  width: 36px;
  height: 36px;
  border: 3px solid var(--border-light, #e8e8e8);
  border-top-color: var(--primary-color, #0078d7);
  border-radius: 50%;
  animation: qr-spin 0.9s linear infinite;
}

@keyframes qr-spin {
  to { transform: rotate(360deg); }
}

.qr-showing {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 16px;
  width: 100%;
}

.qr-image-wrap {
  position: relative;
  padding: 12px;
  background: #fff;
  border-radius: 12px;
  border: 1px solid var(--border-light, #e8e8e8);
  display: flex;
  align-items: center;
  justify-content: center;
}

.qr-image-wrap.expired {
  opacity: 0.5;
}

.qr-image {
  width: 220px;
  height: 220px;
  object-fit: contain;
  display: block;
}

.qr-fallback {
  width: 220px;
  height: 220px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 16px;
  text-align: center;
  font-size: 12px;
  color: var(--text-secondary, #666);
}

.qr-url {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 10px;
  word-break: break-all;
}

.qr-expired-overlay {
  position: absolute;
  inset: 0;
  background: rgba(255, 255, 255, 0.92);
  border-radius: 12px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  font-size: 14px;
  color: var(--text-secondary, #666);
}

.qr-expired-overlay i {
  font-size: 32px;
  color: var(--text-tertiary, #999);
}

.qr-status {
  min-height: 24px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 13px;
  text-align: center;
}

.qr-scanned-btn {
  margin-top: 4px;
  padding: 8px 20px;
  font-size: 13px;
  font-weight: 600;
  background: var(--primary-color, #0078d7);
  color: #fff;
  border: none;
  border-radius: 8px;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  transition: all 0.2s;
}

.qr-scanned-btn:hover {
  background: var(--primary-hover, #0064b8);
  transform: translateY(-1px);
  box-shadow: 0 4px 12px rgba(0, 120, 215, 0.3);
}

.qr-scanned-btn:active {
  transform: translateY(0);
}

.status-text {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  border-radius: 8px;
  font-weight: 500;
}

.status-text.waiting {
  color: var(--text-secondary, #666);
  background: var(--bg-hover, rgba(0, 0, 0, 0.04));
}

.status-text.scanned {
  color: #0078d7;
  background: rgba(0, 120, 215, 0.1);
}

.status-text.authorized {
  color: #28a745;
  background: rgba(40, 167, 69, 0.12);
}

.status-text.expired {
  color: #b8860b;
  background: rgba(255, 193, 7, 0.15);
}

.status-text.error {
  color: #dc3545;
  background: rgba(220, 53, 69, 0.12);
}

.qr-success {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  text-align: center;
}

.qr-success .success-icon {
  font-size: 48px;
  color: #28a745;
}

.qr-success h4 {
  margin: 0;
  font-size: 18px;
  color: var(--text-primary, #333);
}

.qr-success p {
  margin: 0;
  font-size: 13px;
  color: var(--text-secondary, #666);
}

.qr-modal-footer {
  padding: 12px 20px;
  border-top: 1px solid var(--border-light, #e8e8e8);
  text-align: center;
}

.qr-tip {
  font-size: 12px;
  color: var(--text-tertiary, #999);
}

@media (max-width: 480px) {
  .qr-modal {
    max-width: 100%;
  }
  .qr-image,
  .qr-fallback {
    width: 180px;
    height: 180px;
  }
}

/* 登录状态徽章 */
.login-badge {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 2px 8px;
  border-radius: 10px;
  font-size: 11px;
  font-weight: 600;
  margin-left: 6px;
}

.login-badge.ok {
  background: rgba(40, 167, 69, 0.12);
  color: #28a745;
}

.login-badge.warn {
  background: rgba(255, 193, 7, 0.15);
  color: #b8860b;
}
</style>
