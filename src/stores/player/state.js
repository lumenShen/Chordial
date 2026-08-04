/**
 * PlayerStore 共享上下文 — 跨子模块共享的唯一状态与可变引用。
 *
 * 设计目标：
 * - 整个 PlayerStore 只有一个 `reactive` state 对象，保证 `PlayerStore.state.xxx`
 *   路径对所有调用方完全不变（向后兼容）。
 * - 跨子模块的引用（取消令牌、沉浸定时器、actions 命名空间）统一挂在 `shared` 上，
 *   避免子模块循环 import。
 *
 * @module player/state
 */

import { reactive, readonly } from 'vue';

// 播放模式枚举
export const PlayMode = {
  SEQUENCE: 'sequence',  // 顺序播放
  RANDOM: 'random',      // 随机播放
  LOOP: 'loop',          // 循环播放
  LOOP_ONE: 'loop_one',  // 单曲循环
};

// 创建音频元素
export const createAudioElement = () => {
  const audio = new Audio();
  audio.preload = 'metadata';
  // crossOrigin 必须在 src 设置之前指定，否则资源会以 no-cors 模式加载，
  // MediaElementAudioSource 会因 CORS 限制输出全零（频谱分析失效）。
  // chordial:// 协议层已注入 Access-Control-Allow-Origin: *，可放心开启。
  audio.crossOrigin = 'anonymous';
  return audio;
};

// 沉浸模式定时器延迟（5s 无操作进入观赏模式）
export const IMMERSIVE_DELAY = 5000;

// 共享上下文：跨子模块共享的单一状态与可变引用
export const shared = {
  /** 唯一响应式状态（所有子模块共享，勿重建） */
  state: reactive({
    // 当前播放状态
    currentTrack: null,        // 当前播放的歌曲 (Track 实例)
    isPlaying: false,          // 是否正在播放
    currentTime: 0,            // 当前播放时间 (秒)
    duration: 0,               // 总时长 (秒)
    volume: 0.8,               // 音量 (0-1)
    muted: false,              // 是否静音

    // 播放列表
    playlist: [],              // 当前播放列表
    currentIndex: -1,          // 当前歌曲在播放列表中的索引
    playMode: PlayMode.SEQUENCE, // 播放模式

    // 音频元素
    audioElement: null,        // HTMLAudioElement

    // 加载状态
    isLoading: false,          // 是否正在加载
    error: null,               // 错误信息

    // 歌词显示
    showLyrics: false,         // 是否显示歌词页面
    lyricsData: {              // 歌词数据
      plainLyrics: '',
      syncedLyrics: '',
      hasSyncedLyrics: false,
      hasPlainLyrics: false,
    },

    // ── PlayerView UI 状态（模态化，不再通过 router）──────────────
    ui: {
      isPlayerViewOpen: false,          // PlayerView 模态是否打开
      playerViewModeMobile: 'regular',  // 移动端模式: 'details' | 'regular' | 'lyrics'
      playerViewModeDesktop: 'info',    // 桌面端模式: 'info' | 'lyrics' | 'playlist'
      playerViewExpandProgress: 0,      // 0-1, 拖动展开进度（player-control-bar → player-view）
      isImmersive: false,               // 自动观赏模式（隐藏控件）
      currentDevice: 'mobile',          // 'mobile' | 'desktop'（768px 断点）
      sharedElementSource: null,        // markRaw DOM 元素引用，用于 FLIP 共享元素动画
      playerViewTransitioning: false,   // 过渡中，阻止交互
      // 真共享 DOM portal 目标（动态 Teleport）
      coverPortalTarget: null,          // markRaw DOM 元素，PlayerView 内的封面 portal 容器
      metaPortalTarget: null,           // markRaw DOM 元素，PlayerView 内的元信息 portal 容器
      // ── AMLL MenuButton 触发的歌曲菜单 Sheet（bindSheet）────────
      isTrackMenuOpen: false,           // AMLL onRequestOpenMenuAtom 触发的歌曲菜单是否展开
      // ── AMLL bottomControls 播放列表按钮触发的播放队列 Sheet ────
      isPlayQueueOpen: false,           // 横向布局底部第一个按钮（type="playlist"）触发的播放队列是否展开
    },
  }),

  /** play() 取消令牌：快速切歌时旧 play() 通过 token 失效放弃副作用 */
  currentPlayToken: 0,

  /** 沉浸模式定时器（模块级，不放入 reactive state） */
  immersiveTimer: null,

  /** 音频事件处理函数（用于正确移除监听） */
  audioEventHandlers: {
    timeupdate: null,
    loadedmetadata: null,
    durationchange: null,
    canplay: null,
    waiting: null,
    ended: null,
    error: null,
    volumechange: null,
  },

  /** 组装后的 actions 命名空间（由 index.js 注入，供子模块跨调用） */
  actions: null,
};

/** 只读视图（供 index.js 导出为 state） */
export const readonlyState = readonly(shared.state);