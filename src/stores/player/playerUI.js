/**
 * PlayerView UI 状态 — 播放器模态、沉浸模式、portal 目标、Sheet 开关。
 *
 * @module player/playerUI
 */

import { markRaw } from 'vue';
import { shared, IMMERSIVE_DELAY } from './state.js';

/** 打开 PlayerView 模态 */
function openPlayerView(sourceEl = null) {
  const { state } = shared;
  state.ui.sharedElementSource = sourceEl ? markRaw(sourceEl) : null;
  state.ui.isPlayerViewOpen = true;
  state.ui.playerViewTransitioning = true;
  resetImmersiveTimer();
}

/** 关闭 PlayerView 模态 */
function closePlayerView() {
  const { state } = shared;
  state.ui.isPlayerViewOpen = false;
  state.ui.sharedElementSource = null;
  state.ui.isImmersive = false;
  state.ui.playerViewExpandProgress = 0;
  state.ui.playerViewTransitioning = false;
  if (shared.immersiveTimer) {
    clearTimeout(shared.immersiveTimer);
    shared.immersiveTimer = null;
  }
}

/** 标记过渡结束 */
function setPlayerViewTransitioning(v) {
  shared.state.ui.playerViewTransitioning = v;
}

/** 设置共享 DOM portal 目标（用于真共享 DOM 动画） */
function setPortalTargets(coverEl = null, metaEl = null) {
  const { state } = shared;
  state.ui.coverPortalTarget = coverEl ? markRaw(coverEl) : null;
  state.ui.metaPortalTarget = metaEl ? markRaw(metaEl) : null;
}

/** 打开歌曲菜单 Sheet */
function openTrackMenu() {
  shared.state.ui.isTrackMenuOpen = true;
}

/** 关闭歌曲菜单 Sheet */
function closeTrackMenu() {
  shared.state.ui.isTrackMenuOpen = false;
}

/** 打开播放队列 Sheet */
function openPlayQueue() {
  shared.state.ui.isPlayQueueOpen = true;
}

/** 关闭播放队列 Sheet */
function closePlayQueue() {
  shared.state.ui.isPlayQueueOpen = false;
}

/** 设置展开进度（拖动时 0→1） */
function setExpandProgress(p) {
  shared.state.ui.playerViewExpandProgress = Math.max(0, Math.min(1, p));
}

/** 切换 PlayerView 模式 */
function setPlayerViewMode(mode) {
  const { state } = shared;
  if (state.ui.currentDevice === 'desktop') {
    state.ui.playerViewModeDesktop = mode;
  } else {
    state.ui.playerViewModeMobile = mode;
  }
  registerInteraction();
}

/** 进入观赏模式（隐藏控件） */
function enterImmersive() {
  shared.state.ui.isImmersive = true;
}

/** 退出观赏模式（显示控件） */
function exitImmersive() {
  shared.state.ui.isImmersive = false;
  resetImmersiveTimer();
}

/** 注册用户交互（重置沉浸定时器） */
function registerInteraction() {
  if (shared.state.ui.isImmersive) {
    exitImmersive();
  } else {
    resetImmersiveTimer();
  }
}

/** 重置沉浸模式定时器 */
function resetImmersiveTimer() {
  const { state } = shared;
  if (shared.immersiveTimer) {
    clearTimeout(shared.immersiveTimer);
  }
  if (state.ui.isPlayerViewOpen) {
    shared.immersiveTimer = setTimeout(() => {
      enterImmersive();
    }, IMMERSIVE_DELAY);
  }
}

/** 设备检测（768px 断点） */
function detectDevice(width) {
  const newDevice = width >= 768 ? 'desktop' : 'mobile';
  if (shared.state.ui.currentDevice !== newDevice) {
    shared.state.ui.currentDevice = newDevice;
  }
}

export const playerUI = {
  openPlayerView,
  closePlayerView,
  setPlayerViewTransitioning,
  setPortalTargets,
  openTrackMenu,
  closeTrackMenu,
  openPlayQueue,
  closePlayQueue,
  setExpandProgress,
  setPlayerViewMode,
  enterImmersive,
  exitImmersive,
  registerInteraction,
  resetImmersiveTimer,
  detectDevice,
};