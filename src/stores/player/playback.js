/**
 * 播放控制 — 播放/暂停/恢复/切换播放 + 播放核心逻辑 + 取消令牌。
 *
 * @module player/playback
 */

import { markRaw } from 'vue';
import { perf } from '@/utils/performanceMonitor.js';
import { shared } from './state.js';
import { playlist } from './playlist.js';
import { audioEngine } from './audioEngine.js';

/**
 * 播放指定歌曲（核心入口）。
 * 设计：使用取消令牌（myToken = ++currentPlayToken），快速切歌时旧 play()
 * 若在 await 后发现 token 已失效，则放弃所有副作用，避免覆盖新曲目状态。
 *
 * @param {Track} track - 要播放的歌曲
 * @param {Array} playlist - 可选的播放列表
 */
async function play(track, playList = null) {
  return perf.measureAsync('PlayerStore.play', (async () => {
    const { state } = shared;
    if (!track || !track.id) {
      console.warn('play: 无效的 track 参数');
      return;
    }

    // 取消令牌：快速切歌时旧 play() 的 await resolve 后通过 token 失效放弃副作用，
    // 避免旧曲目覆盖新曲目的 audioElement.src / isPlaying / 歌词加载。
    const myToken = ++shared.currentPlayToken;

    try {
      state.isLoading = true;
      state.error = null;

      // 如果提供了播放列表，更新播放列表
      if (playList && Array.isArray(playList)) {
        playlist.setPlaylist(playList);
      }

      // 如果当前已经在播放这首歌，继续播放
      if (state.currentTrack?.id === track.id) {
        if (state.audioElement?.paused) {
          await state.audioElement.play();
          if (myToken !== shared.currentPlayToken) return;
          state.isPlaying = true;
        }
        state.isLoading = false;
        return;
      }

      // 停止当前播放并释放旧的音频资源
      if (state.audioElement) {
        // chordial:// 协议下无需 revokeObjectURL；保留 blob: 兼容
        const oldSrc = state.audioElement.src;
        if (oldSrc && oldSrc.startsWith('blob:')) {
          URL.revokeObjectURL(oldSrc);
        }
        state.audioElement.pause();
        state.audioElement.currentTime = 0;
      }

      // 更新当前歌曲（markRaw 避免 Vue 对 Track 业务类实例创建深代理）
      state.currentTrack = markRaw(track);
      state.currentIndex = playlist.getTrackIndex(track);
      state.currentTime = 0;
      state.duration = track.duration || 0;

      // 获取音频 URL
      const audioUrl = await track.getAudioBlobUrl();
      // 切歌检查：若期间用户又切了别的歌，放弃本次 play 副作用
      if (myToken !== shared.currentPlayToken) return;
      if (!audioUrl) {
        throw new Error('无法获取音频文件');
      }

      // 初始化音频元素（若尚未初始化）
      if (!state.audioElement) {
        audioEngine.initAudioElement();
      }

      // 设置音频源
      state.audioElement.src = audioUrl;

      // 播放（先启动播放，歌词后台加载，不阻塞）
      await state.audioElement.play();
      if (myToken !== shared.currentPlayToken) return;
      state.isPlaying = true;

      // 后台加载歌词，不阻塞播放启动
      shared.actions.loadLyrics(track);

    } catch (error) {
      console.error('播放失败:', error);
      // 已切歌则不污染新曲目的状态
      if (myToken !== shared.currentPlayToken) return;
      state.error = error.message || '播放失败';
      state.isPlaying = false;
    } finally {
      // 仅当仍是当前 token 时才重置 loading，避免旧 play() 提前清掉新 play() 的 loading
      if (myToken === shared.currentPlayToken) {
        state.isLoading = false;
      }
    }
  })());
}

/** 暂停播放 */
function pause() {
  const { state } = shared;
  if (state.audioElement && !state.audioElement.paused) {
    state.audioElement.pause();
    state.isPlaying = false;
  }
}

/** 恢复播放 */
async function resume() {
  const { state } = shared;
  if (state.audioElement && state.audioElement.paused) {
    try {
      await state.audioElement.play();
      state.isPlaying = true;
    } catch (error) {
      console.error('恢复播放失败:', error);
    }
  }
}

/** 切换播放/暂停 */
function togglePlay() {
  if (shared.state.isPlaying) {
    pause();
  } else {
    resume();
  }
}

/** 清理所有资源 */
function dispose() {
  const { state } = shared;
  if (state.audioElement) {
    state.audioElement.pause();
    state.audioElement.src = '';
    audioEngine.cleanupAudioEvents();
    state.audioElement = null;
  }

  // 释放当前歌曲资源
  if (state.currentTrack) {
    state.currentTrack.releaseAudio();
  }

  // 清理沉浸模式定时器
  if (shared.immersiveTimer) {
    clearTimeout(shared.immersiveTimer);
    shared.immersiveTimer = null;
  }

  state.currentTrack = null;
  state.isPlaying = false;
  state.playlist = [];
  state.currentIndex = -1;
  state.ui.isPlayerViewOpen = false;
  state.ui.isImmersive = false;
}

export const playback = {
  play,
  pause,
  resume,
  togglePlay,
  dispose,
};