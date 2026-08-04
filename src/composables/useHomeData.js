/**
 * useHomeData — 首页数据加载 Composable
 *
 * 职责：
 *  - 加载首页统计数据、最近曲目/推荐歌手/最近专辑
 *  - 派生问候语、今日推荐、热门专辑
 *  - 播放处理（播放全部 / 随机播放 / 单曲 / 专辑跳转）
 *  - shallowRef 包装业务类实例，避免 Vue 深代理开销
 *
 * 从原 Home.vue 拆分而来，保持对外行为完全一致。
 */
import { ref, shallowRef, computed, onMounted } from 'vue';
import { useRouter } from 'vue-router';
import { library } from '@/api/musicSource';
import PlayerStore from '@/stores/player/index.js';
import { usePerf } from '@/utils/performanceMonitor.js';

export function useHomeData() {
  const router = useRouter();
  const { start, end, log } = usePerf('Home');

  // shallowRef：列表数据为业务类实例，写入后只读，避免深代理开销
  const recentTracks = shallowRef([]);
  const featuredArtists = shallowRef([]);
  const recentAlbums = shallowRef([]);
  const stats = ref({
    artists: 0,
    albums: 0,
    tracks: 0,
  });
  const isLoading = ref(true);

  // 获取当前时间问候语
  const greeting = computed(() => {
    const hour = new Date().getHours();
    if (hour < 6) return '夜深了';
    if (hour < 12) return '早上好';
    if (hour < 18) return '下午好';
    return '晚上好';
  });

  // 获取今日推荐歌曲（随机选择）
  const todayPicks = computed(() => {
    if (recentTracks.value.length === 0) return [];
    const shuffled = [...recentTracks.value].sort(() => 0.5 - Math.random());
    return shuffled.slice(0, 5);
  });

  // 获取热门专辑（随机选择）
  const featuredAlbums = computed(() => {
    if (recentAlbums.value.length === 0) return [];
    const shuffled = [...recentAlbums.value].sort(() => 0.5 - Math.random());
    return shuffled.slice(0, 4);
  });

  const loadHomeData = async () => {
    isLoading.value = true;
    start('loadHomeData');
    try {
      const data = await library.homeStats();
      if (data) {
        recentTracks.value = data.recentTracks;
        featuredArtists.value = data.featuredArtists;
        recentAlbums.value = data.recentAlbums;
        stats.value = data.stats;
      }
      end('loadHomeData', {
        tracks: recentTracks.value.length,
        artists: featuredArtists.value.length,
        albums: recentAlbums.value.length,
      });
    } catch (error) {
      console.error('Failed to load home data:', error);
      end('loadHomeData', { error: error.message });
    } finally {
      isLoading.value = false;
    }
  };

  onMounted(() => {
    if (recentTracks.value.length === 0) {
      loadHomeData();
    } else {
      isLoading.value = false;
    }
  });

  const handleTrackSelect = (track) => {
    router.push(`/track/${track.id}`);
  };

  const handleAlbumSelect = (album) => {
    router.push(`/albums/${album.id}`);
  };

  const handleTrackPlay = (track) => {
    log('handleTrackPlay', { trackId: track.id, title: track.title });
    PlayerStore.play(track, recentTracks.value);
  };

  const playAllTracks = () => {
    if (recentTracks.value.length > 0) {
      log('playAllTracks', { count: recentTracks.value.length });
      PlayerStore.play(recentTracks.value[0], recentTracks.value);
    }
  };

  const shufflePlay = () => {
    if (recentTracks.value.length > 0) {
      log('shufflePlay', { count: recentTracks.value.length });
      const shuffled = [...recentTracks.value].sort(() => 0.5 - Math.random());
      PlayerStore.play(shuffled[0], shuffled);
    }
  };

  return {
    recentTracks,
    featuredArtists,
    recentAlbums,
    stats,
    isLoading,
    greeting,
    todayPicks,
    featuredAlbums,
    handleTrackSelect,
    handleAlbumSelect,
    handleTrackPlay,
    playAllTracks,
    shufflePlay,
  };
}

export default useHomeData;