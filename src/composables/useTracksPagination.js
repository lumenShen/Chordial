/**
 * useTracksPagination — 歌曲列表分页加载 Composable
 *
 * 职责：
 *  - 维护歌曲列表的分页状态（tracks / totalCount / isLoading / isLoadingMore / hasMore）
 *  - 首屏加载与"加载更多"（无限滚动）逻辑
 *  - loading spinner 动画（anime.js ANIME_LOOP.spin 驱动，替代 CSS @keyframes spin）
 *
 * 用法：
 *   const { run } = useAnime(() => rootRef.value);
 *   const { tracks, totalCount, hasMore, loadTracks, loadMore, playLoadingSpinner } =
 *     useTracksPagination({ run });
 *
 * @param {Object} options
 * @param {Function} options.run - useAnime 暴露的 run 工厂，用于执行动画
 */
import { ref, shallowRef } from 'vue';
import { library } from '@/api/musicSource';
import { usePerf } from '@/utils/performanceMonitor.js';

const PAGE_SIZE = 100;

export function useTracksPagination({ run }) {
  const { start, end } = usePerf('Tracks');

  // shallowRef：列表数据为业务类实例，避免深代理开销
  const tracks = shallowRef([]);
  const totalCount = ref(0);
  const isLoading = ref(true);
  const isLoadingMore = ref(false);
  const hasMore = ref(true);

  // loading spinner：用 ANIME_LOOP.spin 替代 CSS @keyframes spin
  function playLoadingSpinner() {
    run(({ animate, loopPresets }) => {
      animate('.loading-state .spinner', { ...loopPresets.spin });
    });
  }

  const loadTracks = async () => {
    isLoading.value = true;
    start('loadTracks');
    try {
      const data = await library.getSongsPage(0, PAGE_SIZE);
      if (data) {
        tracks.value = data.songs;
        totalCount.value = data.total;
        hasMore.value = data.songs.length < data.total;
      }
      end('loadTracks', { count: tracks.value.length, total: totalCount.value });
    } catch (error) {
      console.error('Failed to load tracks:', error);
      end('loadTracks', { error: error.message });
    } finally {
      isLoading.value = false;
    }
  };

  const loadMore = async () => {
    if (isLoadingMore.value || !hasMore.value) return false;
    isLoadingMore.value = true;
    try {
      const data = await library.getSongsPage(tracks.value.length, PAGE_SIZE);
      if (data) {
        tracks.value = [...tracks.value, ...data.songs];
        hasMore.value = tracks.value.length < data.total;
        // 返回 false 表示已无更多数据；返回 true 表示可能还有更多
        return hasMore.value;
      }
      return false;
    } catch (error) {
      console.error('Failed to load more tracks:', error);
      return false;
    } finally {
      isLoadingMore.value = false;
    }
  };

  return {
    tracks,
    totalCount,
    isLoading,
    isLoadingMore,
    hasMore,
    loadTracks,
    loadMore,
    playLoadingSpinner,
  };
}