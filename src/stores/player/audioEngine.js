/**
 * 音频引擎 — 管理 HTMLAudioElement 的生命周期与事件。
 *
 * 职责：
 * - 创建音频元素、绑定/解绑事件监听
 * - 播放停止、seek、音量、静音
 * - 统一暴露 `getAudioElement()` 供外部（如音频分析器）直接访问真实节点
 *
 * 依赖：`shared` 中共享的唯一 state 与 audioEventHandlers。
 *
 * @module player/audioEngine
 */

import { shared, createAudioElement } from './state.js';

/**
 * 初始化音频元素（首次或重建时清理旧监听）。
 * 由 index.js 在模块加载时调用一次。
 */
export function initAudioElement() {
  const { state } = shared;
  if (state.audioElement) {
    cleanupAudioEvents();
  }

  state.audioElement = createAudioElement();
  state.audioElement.volume = state.volume;
  state.audioElement.muted = state.muted;

  setupAudioEvents();
}

// 设置音频事件监听
function setupAudioEvents() {
  const { state, audioEventHandlers } = shared;
  const audio = state.audioElement;
  if (!audio) return;

  // 时间更新（节流处理，每 100ms 更新一次）
  let lastTimeUpdate = 0;
  const TIME_UPDATE_THROTTLE = 100; // 100ms

  audioEventHandlers.timeupdate = () => {
    const now = performance.now();
    if (now - lastTimeUpdate < TIME_UPDATE_THROTTLE) return;
    lastTimeUpdate = now;

    const _ct = audio.currentTime;
    // 仅在 _ct 为有限数时更新，避免 NaN/Infinity 覆盖 state.currentTime
    // （seek 期间或加载异常时 audio.currentTime 可能短暂为 NaN）
    if (Number.isFinite(_ct)) {
      state.currentTime = _ct;
    }
  };
  audio.addEventListener('timeupdate', audioEventHandlers.timeupdate);

  // 加载完成 — duration 在此时首次可用
  audioEventHandlers.loadedmetadata = () => {
    const d = audio.duration;
    // audio.duration 在某些流式协议下可能为 NaN/Infinity，
    // 此时回退到 track.duration（来自后端元数据），确保 AMLL 进度条 max 不为 0
    if (Number.isFinite(d) && d > 0) {
      state.duration = d;
    } else if (state.currentTrack?.duration && state.currentTrack.duration > 0) {
      state.duration = state.currentTrack.duration;
    }
    state.isLoading = false;
  };
  audio.addEventListener('loadedmetadata', audioEventHandlers.loadedmetadata);

  // duration 变化 — 流式加载时 duration 可能逐步精确（Infinity → 有限值）
  // 必须监听此事件以确保 state.duration 最终拿到真实时长
  audioEventHandlers.durationchange = () => {
    const d = audio.duration;
    if (Number.isFinite(d) && d > 0) {
      state.duration = d;
    }
  };
  audio.addEventListener('durationchange', audioEventHandlers.durationchange);

  // 可以播放
  audioEventHandlers.canplay = () => {
    state.isLoading = false;
  };
  audio.addEventListener('canplay', audioEventHandlers.canplay);

  // 等待加载
  audioEventHandlers.waiting = () => {
    state.isLoading = true;
  };
  audio.addEventListener('waiting', audioEventHandlers.waiting);

  // 播放结束
  audioEventHandlers.ended = () => {
    handleTrackEnded();
  };
  audio.addEventListener('ended', audioEventHandlers.ended);

  // 错误处理
  audioEventHandlers.error = (e) => {
    console.error('Audio playback error:', e);
    state.error = '播放出错';
    state.isLoading = false;
    state.isPlaying = false;
  };
  audio.addEventListener('error', audioEventHandlers.error);

  // 音量变化
  audioEventHandlers.volumechange = () => {
    state.volume = audio.volume;
    state.muted = audio.muted;
  };
  audio.addEventListener('volumechange', audioEventHandlers.volumechange);
}

// 清理音频事件监听
function cleanupAudioEvents() {
  const { state, audioEventHandlers } = shared;
  const audio = state.audioElement;
  if (!audio) return;

  // 移除所有事件监听
  if (audioEventHandlers.timeupdate) {
    audio.removeEventListener('timeupdate', audioEventHandlers.timeupdate);
  }
  if (audioEventHandlers.loadedmetadata) {
    audio.removeEventListener('loadedmetadata', audioEventHandlers.loadedmetadata);
  }
  if (audioEventHandlers.durationchange) {
    audio.removeEventListener('durationchange', audioEventHandlers.durationchange);
  }
  if (audioEventHandlers.canplay) {
    audio.removeEventListener('canplay', audioEventHandlers.canplay);
  }
  if (audioEventHandlers.waiting) {
    audio.removeEventListener('waiting', audioEventHandlers.waiting);
  }
  if (audioEventHandlers.ended) {
    audio.removeEventListener('ended', audioEventHandlers.ended);
  }
  if (audioEventHandlers.error) {
    audio.removeEventListener('error', audioEventHandlers.error);
  }
  if (audioEventHandlers.volumechange) {
    audio.removeEventListener('volumechange', audioEventHandlers.volumechange);
  }
}

// 处理歌曲播放结束
function handleTrackEnded() {
  const { state } = shared;
  switch (state.playMode) {
    case 'loop_one':
      // 单曲循环，重新播放
      state.audioElement.currentTime = 0;
      state.audioElement.play();
      break;
    case 'loop':
    case 'random':
    case 'sequence':
    default:
      shared.actions.playNext();
      break;
  }
}

/** 跳转到指定时间（clamp 上限优先级：audio.duration > state.duration > Infinity） */
function seek(time) {
  const { state } = shared;
  const audio = state.audioElement;
  if (!audio) return;

  // 选择有效的 clamp 上限：优先 audio.duration（真实音频时长），
  // 其次 state.duration（可能来自 track.duration 或已同步的 audio.duration）
  const audioDur = Number.isFinite(audio.duration) ? audio.duration : 0;
  const stateDur = Number.isFinite(state.duration) ? state.duration : 0;
  const effectiveDuration = audioDur > 0 ? audioDur : (stateDur > 0 ? stateDur : Infinity);

  // 只有 time 是有效数字才 clamp；非有限数直接忽略
  let clampedTime;
  if (!Number.isFinite(time)) {
    return;
  }
  clampedTime = effectiveDuration === Infinity
    ? Math.max(0, time)
    : Math.max(0, Math.min(time, effectiveDuration));

  try {
    audio.currentTime = clampedTime;
  } catch (e) {
    console.error('[seek] 设置 currentTime 失败:', e);
  }
  state.currentTime = clampedTime;
}

/** 跳转到进度百分比 */
function seekToPercent(percent) {
  const { state } = shared;
  const time = (percent / 100) * state.duration;
  seek(time);
}

/** 设置音量 */
function setVolume(volume) {
  const { state } = shared;
  const clampedVolume = Math.max(0, Math.min(1, volume));
  state.volume = clampedVolume;
  if (state.audioElement) {
    state.audioElement.volume = clampedVolume;
  }
}

/** 切换静音 */
function toggleMute() {
  const { state } = shared;
  state.muted = !state.muted;
  if (state.audioElement) {
    state.audioElement.muted = state.muted;
  }
}

/** 停止播放 */
function stop() {
  const { state } = shared;
  if (state.audioElement) {
    state.audioElement.pause();
    state.audioElement.currentTime = 0;
  }
  state.isPlaying = false;
  state.currentTime = 0;
}

/** 直接访问真实音频节点（绕过 reactive 深代理，供音频分析器使用） */
function getAudioElement() {
  return shared.state.audioElement;
}

export const audioEngine = {
  initAudioElement,
  cleanupAudioEvents,
  seek,
  seekToPercent,
  setVolume,
  toggleMute,
  stop,
  getAudioElement,
};