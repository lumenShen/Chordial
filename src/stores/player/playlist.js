/**
 * 播放列表 — 管理播放列表的增删改查、播放模式与前后切换。
 *
 * @module player/playlist
 */

import { markRaw } from 'vue';
import { shared, PlayMode } from './state.js';

// 获取歌曲在播放列表中的索引
function getTrackIndex(track) {
  if (!track) return -1;
  return shared.state.playlist.findIndex((t) => t.id === track.id);
}

/** 播放上一首 */
function playPrevious() {
  const { state } = shared;
  if (state.playlist.length === 0) return;

  let previousIndex;

  if (state.playMode === PlayMode.RANDOM) {
    // 随机模式，随机选择一首
    previousIndex = Math.floor(Math.random() * state.playlist.length);
  } else {
    // 其他模式，播放上一首
    previousIndex = state.currentIndex - 1;
    if (previousIndex < 0) {
      previousIndex = state.playlist.length - 1; // 循环到最后一首
    }
  }

  const previousTrack = state.playlist[previousIndex];
  if (previousTrack) {
    shared.actions.play(previousTrack);
  }
}

/** 播放下一首 */
function playNext() {
  const { state } = shared;
  if (state.playlist.length === 0) return;

  let nextIndex;

  if (state.playMode === PlayMode.RANDOM) {
    // 随机模式，随机选择一首
    nextIndex = Math.floor(Math.random() * state.playlist.length);
  } else {
    // 其他模式，播放下一首
    nextIndex = state.currentIndex + 1;
    if (nextIndex >= state.playlist.length) {
      nextIndex = 0; // 循环到第一首
    }
  }

  const nextTrack = state.playlist[nextIndex];
  if (nextTrack) {
    shared.actions.play(nextTrack);
  }
}

/** 随机播放 */
function playRandom() {
  const { state } = shared;
  if (state.playlist.length === 0) return;

  const randomIndex = Math.floor(Math.random() * state.playlist.length);
  const randomTrack = state.playlist[randomIndex];
  if (randomTrack) {
    shared.actions.play(randomTrack);
  }
}

/** 设置播放模式 */
function setPlayMode(mode) {
  if (Object.values(PlayMode).includes(mode)) {
    shared.state.playMode = mode;
  }
}

/** 切换播放模式 */
function togglePlayMode() {
  const modes = Object.values(PlayMode);
  const currentIndex = modes.indexOf(shared.state.playMode);
  const nextIndex = (currentIndex + 1) % modes.length;
  shared.state.playMode = modes[nextIndex];
}

/** 设置播放列表 */
function setPlaylist(playlist) {
  const { state } = shared;
  // markRaw 每首 Track 实例，避免 Vue 对业务类（含方法）创建深代理
  state.playlist = (playlist || []).map((t) => markRaw(t));
  // 更新当前歌曲索引
  if (state.currentTrack) {
    state.currentIndex = getTrackIndex(state.currentTrack);
  }
}

/** 添加歌曲到播放列表 */
function addToPlaylist(track) {
  const { state } = shared;
  if (!track) return;

  // 检查是否已存在
  const exists = state.playlist.some((t) => t.id === track.id);
  if (!exists) {
    state.playlist.push(markRaw(track));
  }
}

/** 从播放列表移除歌曲 */
function removeFromPlaylist(trackId) {
  const { state } = shared;
  const index = state.playlist.findIndex((t) => t.id === trackId);
  if (index !== -1) {
    state.playlist.splice(index, 1);

    // 更新当前索引
    if (state.currentTrack) {
      state.currentIndex = getTrackIndex(state.currentTrack);
    }
  }
}

/** 清空播放列表 */
function clearPlaylist() {
  const { state } = shared;
  state.playlist = [];
  state.currentIndex = -1;
  if (state.isPlaying) {
    shared.actions.stop();
  }
}

export const playlist = {
  getTrackIndex,
  playPrevious,
  playNext,
  playRandom,
  setPlayMode,
  togglePlayMode,
  setPlaylist,
  addToPlaylist,
  removeFromPlaylist,
  clearPlaylist,
};