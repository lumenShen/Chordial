/**
 * useArtistDetail — 歌手详情页数据加载 Composable
 *
 * 职责：
 *  - 加载歌手主数据 + 关联专辑/歌曲（并发）
 *  - 取消令牌：快速切换歌手时旧请求放弃副作用，避免旧数据覆盖新数据
 *  - 响应路由 param 变化（支持同组件复用切换歌手）
 *  - shallowRef 包装 Artist 业务类实例，避免 Vue 深代理开销
 *
 * 与原 ArtistDetail.vue onMounted 的区别：
 *  - onMounted 只在组件首次挂载时触发，路由 param 变化（同组件复用）时不重新加载
 *  - 本 composable watch artistIdRef，param 变化即重新加载
 */
import { ref, shallowRef, watch } from 'vue';
import { getArtist } from '@/api/artist';
import { getAlbumsByIds } from '@/api/album';
import { getSongsByIds } from '@/api/musicSource/musicResource';
import { usePerf } from '@/utils/performanceMonitor.js';

export function useArtistDetail(artistIdRef) {
  // shallowRef：Artist/Album/Song 业务类实例避免深代理
  const artist = shallowRef(null);
  const albums = shallowRef([]);
  const tracks = shallowRef([]);
  const isLoading = ref(true);
  const error = ref(null);

  const { start, end } = usePerf('ArtistDetail');

  // 取消令牌：每次加载递增，await 后比较，不一致则放弃副作用
  let loadToken = 0;

  async function load(artistId) {
    if (!artistId) {
      artist.value = null;
      albums.value = [];
      tracks.value = [];
      isLoading.value = false;
      return;
    }

    const myToken = ++loadToken;
    isLoading.value = true;
    error.value = null;

    try {
      start('loadArtist');
      const artistData = await getArtist(artistId);
      // 切换检查：若期间用户又切了别的歌手，放弃本次副作用
      if (myToken !== loadToken) return;

      artist.value = artistData;

      // 并发加载专辑和歌曲详情
      start('loadArtistDetails');
      const [albumList, trackList] = await Promise.all([
        artistData.albumIds?.length > 0
          ? getAlbumsByIds(artistData.albumIds)
          : Promise.resolve([]),
        artistData.trackIds?.length > 0
          ? getSongsByIds(artistData.trackIds)
          : Promise.resolve([]),
      ]);

      // 再次检查：await 期间可能已切换
      if (myToken !== loadToken) return;

      albums.value = albumList;
      tracks.value = trackList;
      end('loadArtistDetails', {
        albumCount: albumList.length,
        trackCount: trackList.length,
      });
      end('loadArtist', {
        albumIds: artistData?.albumIds?.length ?? 0,
        trackIds: artistData?.trackIds?.length ?? 0,
      });
    } catch (e) {
      if (myToken !== loadToken) return;
      console.error('Failed to load artist:', e);
      error.value = e;
      end('loadArtist', { error: e.message });
    } finally {
      // 仅当仍是当前 token 时才重置 loading，避免旧 load() 提前清掉新 load() 的 loading
      if (myToken === loadToken) {
        isLoading.value = false;
      }
    }
  }

  // 监听 artistId 变化（支持路由 param 切换同组件复用）
  watch(artistIdRef, (newId) => load(newId), { immediate: true });

  return {
    artist,
    albums,
    tracks,
    isLoading,
    error,
    reload: load,
  };
}

export default useArtistDetail;
