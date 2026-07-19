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
          <label class="setting-label">登录状态</label>
          <span class="setting-desc">
            扫码登录后可访问 VIP 歌曲、私人歌单等资源。
            <span v-if="loginStatus.loggedIn" class="login-badge ok">
              <i class="bi bi-check-circle-fill"></i> 已登录<span v-if="loginStatus.nickname">（{{ loginStatus.nickname }}）</span>
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

    <!-- 我的资源（登录后自动加载） -->
    <div v-if="loginStatus.loggedIn" class="settings-section">
      <h3 class="block-title">
        <i class="bi bi-cloud-arrow-down"></i>
        我的资源
        <span v-if="myResources.loading" class="loading-text">
          <i class="bi bi-arrow-clockwise spinning"></i> 加载中…
        </span>
      </h3>

      <div v-if="myResources.error" class="test-result fail">
        <i class="bi bi-x-circle-fill"></i>
        <span>{{ myResources.error }}</span>
        <button class="action-btn ghost" @click="loadMyResources" style="margin-left: auto;">
          <i class="bi bi-arrow-clockwise"></i> 重试
        </button>
      </div>

      <!-- 我创建的歌单 -->
      <div v-if="myResources.createdPlaylists.length" class="my-resource-group">
        <h4 class="group-title">
          <i class="bi bi-music-note-list"></i>
          我创建的歌单
          <span class="group-count">{{ myResources.createdPlaylists.length }}</span>
        </h4>
        <div class="resource-grid">
          <div v-for="item in myResources.createdPlaylists" :key="item.id" class="resource-card">
            <img v-if="item.coverImgUrl" :src="item.coverImgUrl" :alt="item.name" class="resource-cover" loading="lazy" />
            <div v-else class="resource-cover placeholder"><i class="bi bi-music-note-beamed"></i></div>
            <div class="resource-info">
              <div class="resource-name" :title="item.name">{{ item.name }}</div>
              <div class="resource-meta">
                <span>{{ item.trackCount }} 首</span>
                <span v-if="item.playCount">· {{ formatCount(item.playCount) }} 次播放</span>
              </div>
            </div>
            <button
              class="action-btn primary small"
              :class="{ done: myResources.imported.has(item.id) }"
              :disabled="myResources.importing[item.id] === 'loading' || myResources.imported.has(item.id)"
              @click="importPlaylist(item)"
            >
              <i class="bi" :class="
                myResources.importing[item.id] === 'loading' ? 'bi-arrow-clockwise spinning' :
                myResources.imported.has(item.id) ? 'bi-check-lg' :
                myResources.importing[item.id] === 'error' ? 'bi-arrow-clockwise' :
                'bi-download'
              "></i>
              {{ myResources.imported.has(item.id) ? '已导入' : '导入' }}
            </button>
          </div>
        </div>
      </div>

      <!-- 收藏的歌单 -->
      <div v-if="myResources.subscribedPlaylists.length" class="my-resource-group">
        <h4 class="group-title">
          <i class="bi bi-heart"></i>
          收藏的歌单
          <span class="group-count">{{ myResources.subscribedPlaylists.length }}</span>
        </h4>
        <div class="resource-grid">
          <div v-for="item in myResources.subscribedPlaylists" :key="item.id" class="resource-card">
            <img v-if="item.coverImgUrl" :src="item.coverImgUrl" :alt="item.name" class="resource-cover" loading="lazy" />
            <div v-else class="resource-cover placeholder"><i class="bi bi-music-note-beamed"></i></div>
            <div class="resource-info">
              <div class="resource-name" :title="item.name">{{ item.name }}</div>
              <div class="resource-meta">
                <span v-if="item.creator">by {{ item.creator.nickname }}</span>
                <span>· {{ item.trackCount }} 首</span>
              </div>
            </div>
            <button
              class="action-btn primary small"
              :class="{ done: myResources.imported.has(item.id) }"
              :disabled="myResources.importing[item.id] === 'loading' || myResources.imported.has(item.id)"
              @click="importPlaylist(item)"
            >
              <i class="bi" :class="
                myResources.importing[item.id] === 'loading' ? 'bi-arrow-clockwise spinning' :
                myResources.imported.has(item.id) ? 'bi-check-lg' :
                'bi-download'
              "></i>
              {{ myResources.imported.has(item.id) ? '已导入' : '导入' }}
            </button>
          </div>
        </div>
      </div>

      <!-- 收藏的专辑 -->
      <div v-if="myResources.albums.length" class="my-resource-group">
        <h4 class="group-title">
          <i class="bi bi-disc"></i>
          收藏的专辑
          <span class="group-count">{{ myResources.albums.length }} / {{ myResources.albumsTotal }}</span>
        </h4>
        <div class="resource-grid">
          <div v-for="item in myResources.albums" :key="item.id" class="resource-card">
            <img v-if="item.picUrl" :src="item.picUrl" :alt="item.name" class="resource-cover" loading="lazy" />
            <div v-else class="resource-cover placeholder"><i class="bi bi-disc"></i></div>
            <div class="resource-info">
              <div class="resource-name" :title="item.name">{{ item.name }}</div>
              <div class="resource-meta">
                <span v-if="item.artist">{{ item.artist.name }}</span>
                <span>· {{ item.trackCount }} 首</span>
              </div>
            </div>
            <button
              class="action-btn primary small"
              :class="{ done: myResources.imported.has(item.id) }"
              :disabled="myResources.importing[item.id] === 'loading' || myResources.imported.has(item.id)"
              @click="importAlbum(item)"
            >
              <i class="bi" :class="
                myResources.importing[item.id] === 'loading' ? 'bi-arrow-clockwise spinning' :
                myResources.imported.has(item.id) ? 'bi-check-lg' :
                'bi-download'
              "></i>
              {{ myResources.imported.has(item.id) ? '已导入' : '导入' }}
            </button>
          </div>
        </div>
      </div>

      <!-- 三类都为空的提示 -->
      <div
        v-if="!myResources.loading && !myResources.error
          && !myResources.createdPlaylists.length
          && !myResources.subscribedPlaylists.length
          && !myResources.albums.length"
        class="empty-tip"
      >
        <i class="bi bi-inbox"></i>
        <p>未找到任何歌单或收藏的专辑</p>
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

    <!-- 扫码登录 Modal -->
    <teleport to="body">
      <transition name="fade">
        <div v-if="qrLogin.visible" class="qr-modal-mask" @click.self="closeQrLogin">
          <div class="qr-modal" role="dialog" aria-modal="true">
            <div class="qr-modal-header">
              <h3 class="qr-modal-title">
                <i class="bi bi-qr-code"></i> 网易云扫码登录
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
                  <img v-if="qrLogin.qrimg" :src="qrLogin.qrimg" alt="网易云登录二维码" class="qr-image" />
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
                    <i class="bi bi-hourglass-split"></i> 请使用网易云音乐 App 扫码
                  </div>
                  <div v-else-if="qrLogin.status === 'scanned'" class="status-text scanned">
                    <i class="bi bi-phone"></i> 已扫码，请在手机上确认登录
                    <img v-if="qrLogin.avatarUrl" :src="qrLogin.avatarUrl" class="avatar" alt="用户头像" />
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
                <p v-if="qrLogin.nickname">欢迎，{{ qrLogin.nickname }}</p>
                <button class="action-btn primary" @click="closeQrLogin">完成</button>
              </div>
            </div>

            <div class="qr-modal-footer">
              <span class="qr-tip">使用网易云音乐 App 扫描左侧二维码即可登录</span>
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
import { ref, computed, onMounted, onUnmounted, useTemplateRef } from 'vue';
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

// ── 扫码登录 ────────────────────────────────────────────────
const showManualCookie = ref(false);

// 登录状态（仅前端展示用，从 cookie 字符串推断）
const loginStatus = ref({ loggedIn: false, nickname: '' });

// "我的资源"状态
const myResources = ref({
  loading: false,
  error: '',
  uid: 0,
  // 用户创建的歌单
  createdPlaylists: [],
  // 用户收藏的歌单
  subscribedPlaylists: [],
  // 用户收藏的专辑
  albums: [],
  albumsTotal: 0,
  // 各项导入中状态（key 为资源 id，value 为 'loading' | 'done' | 'error'）
  importing: {},
  // 已导入完成的资源 id 集合
  imported: new Set(),
});

function updateLoginStatus() {
  const cookie = (config.value.cookie || '').trim();
  // MUSIC_U 是网易云登录态的核心 cookie
  const hasMusicU = /(^|;\s*)MUSIC_U\s*=/.test(cookie);
  loginStatus.value = {
    loggedIn: hasMusicU,
    nickname: loginStatus.value.nickname || '',
  };
  // 登录态变化时自动拉取"我的资源"
  if (hasMusicU && myResources.value.uid === 0 && !myResources.value.loading) {
    loadMyResources();
  }
  if (!hasMusicU) {
    // 退出登录时清空我的资源
    myResources.value = {
      loading: false,
      error: '',
      uid: 0,
      createdPlaylists: [],
      subscribedPlaylists: [],
      albums: [],
      albumsTotal: 0,
      importing: {},
      imported: new Set(),
    };
  }
}

// 拉取"我的资源"：用户信息 → 并行拉取歌单 + 专辑
async function loadMyResources() {
  myResources.value.loading = true;
  myResources.value.error = '';
  try {
    const accountResp = await neteaseApi.getUserAccount();
    const profile = accountResp.profile;
    if (!profile || !profile.userId) {
      myResources.value.error = '未获取到用户信息（cookie 可能已过期）';
      myResources.value.loading = false;
      return;
    }
    myResources.value.uid = profile.userId;
    loginStatus.value.nickname = profile.nickname || '';

    // 并行拉取歌单 + 专辑
    const [playlists, albumsResp] = await Promise.all([
      neteaseApi.getUserPlaylists(profile.userId),
      neteaseApi.getUserAlbums(50, 0),
    ]);

    // 按 subscribed 字段区分创建/收藏
    myResources.value.createdPlaylists = (playlists || []).filter(
      (p) => p.subscribed === false || p.subscribed === null || p.subscribed === undefined
    );
    myResources.value.subscribedPlaylists = (playlists || []).filter(
      (p) => p.subscribed === true
    );
    myResources.value.albums = albumsResp.albums || [];
    myResources.value.albumsTotal = albumsResp.total || 0;
  } catch (e) {
    myResources.value.error = e.message || String(e);
  } finally {
    myResources.value.loading = false;
  }
}

// 导入歌单到本地库（复用 savePlaylistToLibrary）
async function importPlaylist(item) {
  if (myResources.value.imported.has(item.id)) return;
  if (myResources.value.importing[item.id] === 'loading') return;
  myResources.value.importing[item.id] = 'loading';
  try {
    await neteaseApi.savePlaylistToLibrary(item.id);
    myResources.value.imported.add(item.id);
    myResources.value.importing[item.id] = 'done';
    showToast(`歌单「${item.name}」已导入`, 'success');
  } catch (e) {
    myResources.value.importing[item.id] = 'error';
    showToast(`导入失败：${e.message || e}`, 'error');
  }
}

// 导入专辑到本地库
async function importAlbum(item) {
  if (myResources.value.imported.has(item.id)) return;
  if (myResources.value.importing[item.id] === 'loading') return;
  myResources.value.importing[item.id] = 'loading';
  try {
    const r = await neteaseApi.saveAlbumToLibrary(item.id);
    myResources.value.imported.add(item.id);
    myResources.value.importing[item.id] = 'done';
    showToast(`专辑「${item.name}」已导入 ${r.imported_count}/${r.total_count} 首`, 'success');
  } catch (e) {
    myResources.value.importing[item.id] = 'error';
    showToast(`导入失败：${e.message || e}`, 'error');
  }
}

// 播放数格式化（万/亿）
function formatCount(n) {
  if (!n || n <= 0) return '0';
  if (n >= 100000000) return (n / 100000000).toFixed(1) + '亿';
  if (n >= 10000) return (n / 10000).toFixed(1) + '万';
  return String(n);
}

// 扫码登录 Modal 状态
const qrLogin = ref({
  visible: false,
  // phase: loading（正在获取二维码）| showing（已显示二维码，轮询中）| polling（同 showing，语义保留）| success
  phase: 'loading',
  // status: waiting | scanned | authorized | expired | unknown
  status: 'waiting',
  key: '',
  qrurl: '',
  qrimg: '',
  avatarUrl: '',
  nickname: '',
  error: '',
});

let pollAbort = false;  // 用户关闭 modal 时置 true，停止轮询

async function openQrLogin() {
  qrLogin.value = {
    visible: true,
    phase: 'loading',
    status: 'waiting',
    key: '',
    qrurl: '',
    qrimg: '',
    avatarUrl: '',
    nickname: '',
    error: '',
  };
  pollAbort = false;
  await fetchQr();
}

async function fetchQr() {
  try {
    const key = await neteaseApi.loginQrKey();
    qrLogin.value.key = key;
    const data = await neteaseApi.loginQrCreate(key, true);
    qrLogin.value.qrurl = data.qrurl || '';
    qrLogin.value.qrimg = data.qrimg || '';
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

// ── 轮询循环（while + pollingActive 标志，替代 setTimeout 链） ──────────────
//
// 之前用 setTimeout 链式调用存在竞争条件（pollTimer 被多个异步函数共享），
// 导致第一次 check 后无法继续轮询。改用 while 循环 + 布尔标志，逻辑更清晰：
// - startPolling() 启动一个异步循环
// - stopPolling() 设置 pollingActive=false，循环在下个 await 点退出
// - 不依赖 pollTimer 变量，避免覆盖/丢失
let pollingActive = false;

function startPolling() {
  if (pollingActive) return;
  pollingActive = true;
  // 启动异步循环（不 await，让调用方立即返回）
  pollingLoop();
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function pollingLoop() {
  // 先等 1 秒再发起第一次 check（避免和 create 抢请求）
  await sleep(1000);

  while (pollingActive && qrLogin.value.visible && !pollAbort) {
    const key = qrLogin.value.key;
    if (!key) {
      // key 还没准备好，等一下再试
      await sleep(500);
      continue;
    }

    try {
      const resp = await neteaseApi.loginQrCheck(key);
      // 请求返回后再次检查轮询是否已停止（用户可能已关闭 modal）
      if (!pollingActive || !qrLogin.value.visible || pollAbort) break;

      qrLogin.value.status = resp.status;
      if (resp.avatarUrl) qrLogin.value.avatarUrl = resp.avatarUrl;
      if (resp.nickname) qrLogin.value.nickname = resp.nickname;

      if (resp.status === 'authorized') {
        // 后端已自动持久化 cookie，重新拉取配置以更新前端状态
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
      if (resp.status === 'expired') {
        // 二维码过期，停止轮询，等用户点"重新获取"
        break;
      }
      // waiting / scanned / unknown → 继续循环，等 1 秒后再 check
      await sleep(1000);
    } catch (e) {
      // 网络错误也继续重试，避免短暂抖动中断流程
      qrLogin.value.error = e.message || String(e);
      console.warn('[qr poll] check 失败，2s 后重试:', e);
      await sleep(2000);
    }
  }

  pollingActive = false;
}

// 用户手动点击"我已扫码"按钮：
// 1. 立即触发一次 check（避免等待下次 1s 轮询）
// 2. 乐观更新 UI 到 "scanned" 状态，给用户即时反馈
// 注意：网易云真实状态由服务端推送，此按钮不能伪造授权态，
//      仅加速 check 并提供视觉反馈。
async function manualScanned() {
  if (qrLogin.value.status !== 'waiting') return;
  // 乐观更新：UI 立即切换到"已扫码待确认"
  qrLogin.value.status = 'scanned';
  // 立即发起一次 check（复用轮询循环里的一段逻辑，但不进入循环）
  const key = qrLogin.value.key;
  if (!key) return;
  try {
    const resp = await neteaseApi.loginQrCheck(key);
    if (!qrLogin.value.visible) return;
    qrLogin.value.status = resp.status;
    if (resp.avatarUrl) qrLogin.value.avatarUrl = resp.avatarUrl;
    if (resp.nickname) qrLogin.value.nickname = resp.nickname;
    if (resp.status === 'authorized') {
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
    // 其他状态交给正在运行的轮询循环继续处理
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

onUnmounted(() => {
  // 组件销毁时停止轮询（设置 pollAbort 让正在 await 的循环也能退出）
  pollAbort = true;
  stopPolling();
});

// ── 初始化 ──────────────────────────────────────────────────
onMounted(async () => {
  await Promise.all([loadConfig(), loadPreference()]);
  updateLoginStatus();
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

.action-btn.ghost {
  background: transparent;
  border-color: var(--border-color, #e8e8e8);
  color: var(--text-secondary, #666);
  font-size: 12px;
  padding: 6px 12px;
}

.action-btn.small {
  padding: 4px 10px;
  font-size: 12px;
  gap: 4px;
}

.action-btn.done {
  background: rgba(40, 167, 69, 0.12);
  color: #28a745;
  border-color: rgba(40, 167, 69, 0.3);
}

.loading-text {
  font-size: 12px;
  color: var(--text-secondary, #666);
  font-weight: normal;
  margin-left: 8px;
}

/* ── 我的资源面板 ───────────────────────────────────────────────────── */
.my-resource-group {
  margin-top: 18px;
}

.group-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-secondary, #666);
  margin: 0 0 10px;
  display: flex;
  align-items: center;
  gap: 6px;
}

.group-count {
  background: var(--bg-tertiary, rgba(15, 15, 15, 0.05));
  color: var(--text-tertiary, #999);
  padding: 1px 8px;
  border-radius: 10px;
  font-size: 11px;
  font-weight: 600;
}

.resource-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
  gap: 10px;
}

.resource-card {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px;
  background: var(--bg-secondary, #fafafa);
  border: 1px solid var(--border-light, #e8e8e8);
  border-radius: 10px;
  transition: all 0.2s;
}

.resource-card:hover {
  border-color: var(--primary-color, #0078d7);
  background: var(--bg-hover, rgba(0, 120, 215, 0.04));
}

.resource-cover {
  width: 56px;
  height: 56px;
  border-radius: 8px;
  object-fit: cover;
  flex-shrink: 0;
  background: var(--bg-tertiary, #f0f0f0);
}

.resource-cover.placeholder {
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-tertiary, #999);
  font-size: 22px;
}

.resource-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.resource-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary, #333);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.resource-meta {
  font-size: 11px;
  color: var(--text-tertiary, #999);
  display: flex;
  gap: 4px;
  flex-wrap: wrap;
}

.empty-tip {
  padding: 32px 16px;
  text-align: center;
  color: var(--text-tertiary, #999);
  font-size: 13px;
}

.empty-tip i {
  font-size: 32px;
  display: block;
  margin-bottom: 8px;
}

.empty-tip p {
  margin: 0;
}

@media (max-width: 480px) {
  .resource-grid {
    grid-template-columns: 1fr;
  }
}

/* ── 扫码登录 Modal（teleport 到 body，使用非 scoped 样式） ─────────── */
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

/* "我已扫码"按钮：仅在 waiting 状态显示，乐观切到 scanned */
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

.status-text.scanned .avatar {
  width: 24px;
  height: 24px;
  border-radius: 50%;
  object-fit: cover;
  margin-left: 4px;
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
</style>
