/**
 * PlayerStore - 全局播放器状态管理（组合入口）。
 *
 * 由职责单一的子模块组装而成：
 * - `state.js`       —— 唯一响应式状态 + 播放模式 + 共享上下文
 * - `audioEngine.js` —— 音频元素生命周期 / 事件 / seek / 音量 / 静音 / 停止
 * - `playlist.js`    —— 播放列表 / 播放模式 / 前后切换
 * - `playback.js`    —— 播放核心 / 取消令牌 / 暂停 / 恢复 / 切换
 * - `lyrics.js`      —— 歌词加载 / 显示
 * - `playerUI.js`    —— PlayerView 模态 / 沉浸模式 / portal / Sheet
 *
 * 对外 API 与旧版 `player.js` 完全兼容：
 * - `PlayerStore.state.xxx`（readonly reactive 对象，路径不变）
 * - `PlayerStore.<getter>.value`（其余 getter 为 computed ref）
 * - `PlayerStore.<action>()`（函数）
 * - `PlayerStore.PlayMode`、`PlayerStore.getAudioElement()`
 */

import { computed } from 'vue';
import { shared, readonlyState, PlayMode } from './state.js';
import { audioEngine } from './audioEngine.js';
import { playlist } from './playlist.js';
import { playback } from './playback.js';
import { lyrics } from './lyrics.js';
import { playerUI } from './playerUI.js';
import { formatDuration as formatTime } from '@/utils/format.js';

// 组装 actions 命名空间（供子模块跨调用，如 audioEngine → playNext）
const actions = {
  ...playback,
  ...playlist,
  ...lyrics,
  ...playerUI,
  ...audioEngine,
};
shared.actions = actions;

// 计算属性
const getters = {
  // 是否有正在播放的歌曲
  hasCurrentTrack: computed(() => shared.state.currentTrack !== null),

  // 播放进度百分比
  progress: computed(() => {
    if (shared.state.duration === 0) return 0;
    return (shared.state.currentTime / shared.state.duration) * 100;
  }),

  // 格式化后的当前时间
  formattedCurrentTime: computed(() => formatTime(shared.state.currentTime)),

  // 格式化后的总时长
  formattedDuration: computed(() => formatTime(shared.state.duration)),

  // 是否可以播放上一首
  canPlayPrevious: computed(() => {
    if (shared.state.playlist.length === 0) return false;
    if (shared.state.playMode === PlayMode.RANDOM) return true;
    return shared.state.currentIndex > 0;
  }),

  // 是否可以播放下一首
  canPlayNext: computed(() => {
    if (shared.state.playlist.length === 0) return false;
    if (shared.state.playMode === PlayMode.RANDOM) return true;
    return shared.state.currentIndex < shared.state.playlist.length - 1;
  }),

  // 播放列表是否为空
  isPlaylistEmpty: computed(() => shared.state.playlist.length === 0),

  // 播放列表长度
  playlistLength: computed(() => shared.state.playlist.length),

  // ── PlayerView UI getters ────────────────────────────────────
  isPlayerViewOpen: computed(() => shared.state.ui.isPlayerViewOpen),
  playerViewMode: computed(() =>
    shared.state.ui.currentDevice === 'desktop'
      ? shared.state.ui.playerViewModeDesktop
      : shared.state.ui.playerViewModeMobile
  ),
  isImmersive: computed(() => shared.state.ui.isImmersive),
  currentDevice: computed(() => shared.state.ui.currentDevice),
  playerViewTransitioning: computed(() => shared.state.ui.playerViewTransitioning),
  coverPortalTarget: computed(() => shared.state.ui.coverPortalTarget),
  metaPortalTarget: computed(() => shared.state.ui.metaPortalTarget),
  isTrackMenuOpen: computed(() => shared.state.ui.isTrackMenuOpen),
  isPlayQueueOpen: computed(() => shared.state.ui.isPlayQueueOpen),
};

// 初始化音频元素（模块加载时）
audioEngine.initAudioElement();

// 导出 PlayerStore
export const PlayerStore = {
  // 只读状态
  state: readonlyState,

  // Getters
  ...getters,

  // Actions
  ...actions,

  // 播放模式枚举
  PlayMode,

  // ── 原始资源访问器（不进入响应式系统）──────────────────────────
  getAudioElement() {
    return audioEngine.getAudioElement();
  },
};

// 默认导出
export default PlayerStore;

// 重新导出播放模式枚举（保持旧 player.js 的命名导出兼容）
export { PlayMode };