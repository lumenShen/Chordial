/**
 * 歌词 — 歌词数据加载与显示状态。
 *
 * @module player/lyrics
 */

import { perf } from '@/utils/performanceMonitor.js';
import { shared } from './state.js';

/** 加载歌词（后台异步，不阻塞播放） */
async function loadLyrics(track) {
  return perf.measureAsync('PlayerStore.loadLyrics', (async () => {
    const { state } = shared;
    try {
      const lyricsInfo = await track.getLyricsInfo();

      // 比较是否有变化，避免不必要的更新
      const hasChanged =
        state.lyricsData.plainLyrics !== lyricsInfo.plainLyrics ||
        state.lyricsData.syncedLyrics !== lyricsInfo.syncedLyrics ||
        state.lyricsData.hasSyncedLyrics !== lyricsInfo.hasSyncedLyrics ||
        state.lyricsData.hasPlainLyrics !== lyricsInfo.hasPlainLyrics;

      if (hasChanged) {
        // 逐个更新字段，而不是替换整个对象
        state.lyricsData.plainLyrics = lyricsInfo.plainLyrics;
        state.lyricsData.syncedLyrics = lyricsInfo.syncedLyrics;
        state.lyricsData.hasSyncedLyrics = lyricsInfo.hasSyncedLyrics;
        state.lyricsData.hasPlainLyrics = lyricsInfo.hasPlainLyrics;
      }
    } catch (error) {
      console.warn('加载歌词失败:', error);
      // 只有当当前有内容时才清空
      if (state.lyricsData.plainLyrics || state.lyricsData.syncedLyrics) {
        state.lyricsData.plainLyrics = '';
        state.lyricsData.syncedLyrics = '';
        state.lyricsData.hasSyncedLyrics = false;
        state.lyricsData.hasPlainLyrics = false;
      }
    }
  })());
}

/** 切换歌词显示 */
function toggleLyrics() {
  shared.state.showLyrics = !shared.state.showLyrics;
}

/** 设置歌词显示状态 */
function setShowLyrics(show) {
  shared.state.showLyrics = show;
}

export const lyrics = {
  loadLyrics,
  toggleLyrics,
  setShowLyrics,
};