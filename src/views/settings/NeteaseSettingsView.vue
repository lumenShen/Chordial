<template>
  <div class="netease-view settings-view">
    <div class="section-header">
      <h2 class="section-title">网易云音乐来源</h2>
      <p class="section-subtitle">配置 NeteaseCloudMusicApi 服务器以搜索/下载在线音乐</p>
    </div>

    <!-- 来源配置 -->
    <div class="settings-section">
      <h3 class="block-title">
        <HIcon name="cloud-arrow-down" />
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
          <label class="setting-label">登录状态</label>
          <span class="setting-desc">
            扫码登录后可访问 VIP 歌曲、私人歌单等资源。
            <span v-if="loginStatus.loggedIn" class="login-badge ok">
              <HIcon name="check-circle-fill" /> 已登录<span v-if="loginStatus.nickname">（{{ loginStatus.nickname }}）</span>
            </span>
            <span v-else class="login-badge warn">
              <HIcon name="exclamation-circle-fill" /> 未登录
            </span>
          </span>
        </div>
        <div class="login-actions">
          <button class="action-btn primary" @click="openQrLogin">
            <HIcon name="qr-code" /> 扫码登录
          </button>
          <button v-if="loginStatus.loggedIn" class="action-btn" @click="logout">
            <HIcon name="box-arrow-right" /> 退出登录
          </button>
          <button class="action-btn ghost" @click="showManualCookie = !showManualCookie">
            <i class="bi bi" :class="showManualCookie ? 'bi-chevron-up' : 'bi-chevron-down'"></i>
            手动填入 Cookie
          </button>
        </div>
        <textarea
          v-if="showManualCookie"
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
            <HIcon name="folder" /> 选择
          </button>
        </div>
      </div>

      <div class="setting-actions">
        <button class="action-btn primary" @click="saveConfig" :disabled="saving">
          <HIcon name="save" /> {{ saving ? '保存中…' : '保存配置' }}
        </button>
        <button class="action-btn" @click="testConnection" :disabled="testing">
            <HIcon name="wifi" :class="{ spinning: testing }" />
          {{ testing ? '测试中…' : '测试连接' }}
        </button>
      </div>

      <div v-if="testResult" class="test-result" :class="{ ok: testResult.ok, fail: !testResult.ok }">
        <i class="bi" :class="testResult.ok ? 'bi-check-circle-fill' : 'bi-x-circle-fill'"></i>
        <span v-if="testResult.ok">连接成功（{{ testResult.base_url }}）</span>
        <span v-else>连接失败：{{ testResult.error || '未知错误' }}</span>
      </div>
    </div>

    <!-- 我的资源（登录后自动加载） -->
    <NeteaseMyResources
      v-if="loginStatus.loggedIn"
      :config="config"
      @notify="onNotify"
      @nickname="onNickname"
    />

    <!-- 歌词优先级 -->
    <NeteaseLyricPriority :pref="pref" @save="savePreference" />

    <!-- 扫码登录 Modal -->
    <NeteaseQrLoginModal
      :visible="qrVisible"
      @close="onQrClose"
      @success="onQrSuccess"
      @notify="onNotify"
    />

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
import { ref, computed, onMounted } from 'vue';
import * as neteaseApi from '@/api/musicSource/netease.js';
import * as prefApi from '@/api/musicSource/lyricPreference.js';
import NeteaseMyResources from '@/components/settings/NeteaseMyResources.vue';
import NeteaseLyricPriority from '@/components/settings/NeteaseLyricPriority.vue';
import NeteaseQrLoginModal from '@/components/settings/NeteaseQrLoginModal.vue';

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

// ── 登录状态（仅前端展示用，从 cookie 字符串推断） ──────────
const loginStatus = ref({ loggedIn: false, nickname: '' });

function updateLoginStatus() {
  const cookie = (config.value.cookie || '').trim();
  // MUSIC_U 是网易云登录态的核心 cookie
  const hasMusicU = /(^|;\s*)MUSIC_U\s*=/.test(cookie);
  loginStatus.value = {
    loggedIn: hasMusicU,
    nickname: loginStatus.value.nickname || '',
  };
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

// ── 扫码登录 ────────────────────────────────────────────────
const showManualCookie = ref(false);
const qrVisible = ref(false);

function openQrLogin() {
  qrVisible.value = true;
}

function onQrClose() {
  qrVisible.value = false;
}

// 登录成功：后端已自动持久化 cookie，重新拉取配置以更新前端状态
async function onQrSuccess() {
  await loadConfig();
  updateLoginStatus();
}

async function logout() {
  if (!confirm('确定退出网易云登录？清除 Cookie 后将无法访问 VIP 歌曲/私人歌单。')) return;
  config.value.cookie = '';
  try {
    await neteaseApi.updateConfig(config.value);
    updateLoginStatus();
    showToast('已退出登录', 'info');
  } catch (e) {
    showToast(`退出失败：${e.message || e}`, 'error');
  }
}

// ── 子组件事件转发 ──────────────────────────────────────────
function onNotify(message, type) {
  showToast(message, type);
}

function onNickname(nickname) {
  loginStatus.value.nickname = nickname;
}

// ── 初始化 ──────────────────────────────────────────────────
onMounted(async () => {
  await Promise.all([loadConfig(), loadPreference()]);
  updateLoginStatus();
});
</script>

<style scoped>
.setting-desc a {
  color: var(--primary-color);
  text-decoration: none;
}

.setting-desc a:hover {
  text-decoration: underline;
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

/* ── 登录状态徽标 + 按钮 ──────────────────────────────────────────── */
.login-badge {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  margin-left: 6px;
  padding: 2px 8px;
  border-radius: 10px;
  font-size: 12px;
  font-weight: 600;
}
.login-badge.ok {
  background: rgba(40, 167, 69, 0.12);
  color: #28a745;
}
.login-badge.warn {
  background: rgba(255, 193, 7, 0.15);
  color: #b8860b;
}

.login-actions {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}
</style>