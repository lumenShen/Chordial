<template>
  <teleport to="body">
    <transition name="fade">
      <div v-if="visible" class="qr-modal-mask" @click.self="close">
        <div class="qr-modal" role="dialog" aria-modal="true">
          <div class="qr-modal-header">
            <h3 class="qr-modal-title">
              <HIcon name="qr-code" /> 网易云扫码登录
            </h3>
            <button class="qr-modal-close" @click="close" aria-label="关闭">
              <HIcon name="x-lg" />
            </button>
          </div>

          <div class="qr-modal-body">
            <!-- loading: 正在获取二维码 -->
            <div v-if="state.phase === 'loading'" class="qr-loading">
              <div class="qr-spinner"></div>
              <p>正在获取二维码…</p>
            </div>

            <!-- 二维码展示 -->
            <div v-else-if="state.phase === 'showing' || state.phase === 'polling'" class="qr-showing">
              <div class="qr-image-wrap" :class="{ expired: state.status === 'expired' }">
                <img v-if="state.qrimg" :src="state.qrimg" alt="网易云登录二维码" class="qr-image" />
                <div v-else class="qr-fallback">
                  <p>无法显示二维码图片</p>
                  <p class="qr-url">{{ state.qrurl }}</p>
                </div>
                <!-- 过期遮罩 -->
                <div v-if="state.status === 'expired'" class="qr-expired-overlay">
                  <HIcon name="clock-history" />
                  <p>二维码已过期</p>
                  <button class="action-btn primary" @click="refreshQr">
                    <HIcon name="arrow-clockwise" /> 重新获取
                  </button>
                </div>
              </div>

              <div class="qr-status">
                <div v-if="state.status === 'waiting'" class="status-text waiting">
                  <HIcon name="hourglass-split" /> 请使用网易云音乐 App 扫码
                </div>
                <div v-else-if="state.status === 'scanned'" class="status-text scanned">
                  <HIcon name="phone" /> 已扫码，请在手机上确认登录
                  <img v-if="state.avatarUrl" :src="state.avatarUrl" class="avatar" alt="用户头像" />
                </div>
                <div v-else-if="state.status === 'authorized'" class="status-text authorized">
                  <HIcon name="check-circle-fill" /> 授权成功
                </div>
                <div v-else-if="state.status === 'expired'" class="status-text expired">
                  <HIcon name="clock-history" /> 二维码已过期，请重新获取
                </div>
                <div v-else-if="state.error" class="status-text error">
                  <HIcon name="x-circle-fill" /> {{ state.error }}
                </div>
              </div>

              <!-- "我已扫码"按钮：仅在 waiting 状态显示，乐观切到 scanned 并立即 check -->
              <button
                v-if="state.status === 'waiting'"
                class="action-btn primary qr-scanned-btn"
                @click="manualScanned"
              >
                <HIcon name="phone" /> 我已扫码
              </button>
            </div>

            <!-- 成功 -->
            <div v-else-if="state.phase === 'success'" class="qr-success">
              <HIcon name="check-circle-fill" class="success-icon" />
              <h4>登录成功</h4>
              <p v-if="state.nickname">欢迎，{{ state.nickname }}</p>
              <button class="action-btn primary" @click="close">完成</button>
            </div>
          </div>

          <div class="qr-modal-footer">
            <span class="qr-tip">使用网易云音乐 App 扫描左侧二维码即可登录</span>
          </div>
        </div>
      </div>
    </transition>
  </teleport>
</template>

<script setup>
import { ref, watch, onUnmounted } from 'vue';
import * as neteaseApi from '@/api/musicSource/netease.js';

const props = defineProps({
  visible: { type: Boolean, default: false },
});

const emit = defineEmits(['close', 'success', 'notify']);

// 扫码登录 Modal 状态
const state = ref({
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

let pollAbort = false; // 用户关闭 modal 时置 true，停止轮询
let pollingActive = false; // 轮询循环运行标志

function resetState() {
  state.value = {
    phase: 'loading',
    status: 'waiting',
    key: '',
    qrurl: '',
    qrimg: '',
    avatarUrl: '',
    nickname: '',
    error: '',
  };
}

async function fetchQr() {
  try {
    const key = await neteaseApi.loginQrKey();
    state.value.key = key;
    const data = await neteaseApi.loginQrCreate(key, true);
    state.value.qrurl = data.qrurl || '';
    state.value.qrimg = data.qrimg || '';
    state.value.phase = 'showing';
    state.value.status = 'waiting';
    startPolling();
  } catch (e) {
    state.value.error = e.message || String(e);
    emit('notify', `获取二维码失败：${e.message || e}`, 'error');
  }
}

function refreshQr() {
  stopPolling();
  state.value.error = '';
  state.value.status = 'waiting';
  state.value.phase = 'loading';
  state.value.qrimg = '';
  state.value.qrurl = '';
  fetchQr();
}

// ── 轮询循环（while + pollingActive 标志，替代 setTimeout 链） ──────────────
//
// 之前用 setTimeout 链式调用存在竞争条件（pollTimer 被多个异步函数共享），
// 导致第一次 check 后无法继续轮询。改用 while 循环 + 布尔标志，逻辑更清晰：
// - startPolling() 启动一个异步循环
// - stopPolling() 设置 pollingActive=false，循环在下个 await 点退出
// - 不依赖 pollTimer 变量，避免覆盖/丢失
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

  while (pollingActive && props.visible && !pollAbort) {
    const key = state.value.key;
    if (!key) {
      // key 还没准备好，等一下再试
      await sleep(500);
      continue;
    }

    try {
      const resp = await neteaseApi.loginQrCheck(key);
      // 请求返回后再次检查轮询是否已停止（用户可能已关闭 modal）
      if (!pollingActive || !props.visible || pollAbort) break;

      state.value.status = resp.status;
      if (resp.avatarUrl) state.value.avatarUrl = resp.avatarUrl;
      if (resp.nickname) state.value.nickname = resp.nickname;

      if (resp.status === 'authorized') {
        // 后端已自动持久化 cookie，交由父组件重新拉取配置以更新前端状态
        state.value.phase = 'success';
        emit('success');
        emit('notify', '登录成功', 'success');
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
      state.value.error = e.message || String(e);
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
  if (state.value.status !== 'waiting') return;
  // 乐观更新：UI 立即切换到"已扫码待确认"
  state.value.status = 'scanned';
  // 立即发起一次 check（复用轮询循环里的一段逻辑，但不进入循环）
  const key = state.value.key;
  if (!key) return;
  try {
    const resp = await neteaseApi.loginQrCheck(key);
    if (!props.visible) return;
    state.value.status = resp.status;
    if (resp.avatarUrl) state.value.avatarUrl = resp.avatarUrl;
    if (resp.nickname) state.value.nickname = resp.nickname;
    if (resp.status === 'authorized') {
      state.value.phase = 'success';
      stopPolling();
      emit('success');
      emit('notify', '登录成功', 'success');
    }
    // 其他状态交给正在运行的轮询循环继续处理
  } catch (e) {
    state.value.error = e.message || String(e);
  }
}

function stopPolling() {
  pollingActive = false;
}

function close() {
  pollAbort = true;
  stopPolling();
  emit('close');
}

// 打开时重置状态并获取二维码
watch(
  () => props.visible,
  (val) => {
    if (val) {
      pollAbort = false;
      resetState();
      fetchQr();
    }
  }
);

onUnmounted(() => {
  // 组件销毁时停止轮询（设置 pollAbort 让正在 await 的循环也能退出）
  pollAbort = true;
  stopPolling();
});
</script>

<style scoped>
/* 淡入淡出过渡（与父页面 toast 一致） */
.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.2s, transform 0.2s;
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
  transform: translate(-50%, 8px);
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
  color: var(--primary-color);
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
  border-top-color: var(--primary-color);
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
  background: var(--primary-color);
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
  box-shadow: 0 4px 12px rgba(0, 122, 255, 0.3);
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
  color: var(--primary-color);
  background: var(--primary-light);
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