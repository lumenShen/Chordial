/**
 * usePlaylistSidebar - 侧边栏歌单区（左栏）编排逻辑
 *
 * 集中管理三个区段（无尽播放 / 播放列表 / 最近播放）的状态与交互，
 * 以及轻提示、歌单创建、电台启动等共享逻辑。由 PlaylistSidebar.vue
 * 调用一次，将响应式状态分发给各子组件，保持对外行为与 store/路由/API
 * 交互完全不变。
 */
import { ref, shallowRef, computed, watch, onMounted } from 'vue';
import { useRouter, useRoute } from 'vue-router';
import PlayerStore from '@/stores/player/index.js';
import * as playlistApi from '@/api/musicSource/playlists.js';
import * as neteaseRadio from '@/api/musicSource/neteaseRadio.js';
import { useMusicSources } from '@/composables/useMusicSources.js';
import { useEndlessPlay } from '@/composables/useEndlessPlay.js';
import { useFavorites, getRecentTracks, clearRecentPlays, recordPlay } from '@/composables/useFavorites.js';

export function usePlaylistSidebar() {
  const router = useRouter();
  const route = useRoute();

  const { sources, refreshSources } = useMusicSources();
  const endless = useEndlessPlay();

  // 网易云是否启用
  const neteaseEnabled = computed(() => {
    const s = (sources.value || []).find((x) => x.id === 'netease');
    return !!s?.enabled;
  });

  // ── 折叠区段状态 ───────────────────────────────────────
  const sectionOpen = ref({
    endless: true,
    playlists: true,
    recent: true,
  });

  // ── 播放列表 ───────────────────────────────────────────
  const playlists = shallowRef([]);
  async function loadPlaylists() {
    try {
      playlists.value = await playlistApi.list();
    } catch (e) {
      /* ignore */
    }
  }

  // ── 最近播放 ───────────────────────────────────────────
  const recentTracks = shallowRef([]);
  async function loadRecent() {
    recentTracks.value = await getRecentTracks(30);
  }
  const { load } = useFavorites();

  // 监听路由与库变更刷新歌单
  watch(() => route.path, () => {
    if (route.path === '/playlists' || route.path.startsWith('/playlists/')) {
      loadPlaylists();
    }
  });

  // 最近播放数据变化时刷新
  watch(() => PlayerStore.state.currentTrack, (track) => {
    if (track?.id) {
      recordPlay(track);
      loadRecent();
    }
  });

  onMounted(async () => {
    refreshSources();
    loadPlaylists();
    await load();
    await loadRecent();
  });

  // ── 无尽播放 / 电台 交互状态 ───────────────────────────
  const busyItem = ref('');       // 正在启动的项目 key
  const toast = ref('');
  let toastTimer = null;
  function showToast(msg) {
    toast.value = msg;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => { toast.value = ''; }, 2600);
  }

  async function runStart(key, fn) {
    busyItem.value = key;
    try {
      const n = await fn();
      showToast(`已开始播放（队列 ${n} 首）`);
    } catch (e) {
      showToast(e?.message || '启动失败');
    } finally {
      busyItem.value = '';
    }
  }

  const startSmartMix = () => runStart('smart-mix', () => endless.start('smart-mix'));
  const startShuffle = () => runStart('shuffle', () => endless.start('shuffle'));

  // 私人 FM
  const startPersonalFm = () =>
    runStart('personal-fm', async () => {
      const songs = await neteaseRadio.personalFm(10);
      if (!songs.length) throw new Error('私人 FM 返回为空');
      PlayerStore.setPlaylist(songs);
      PlayerStore.play(songs[0], songs);
      return songs.length;
    });

  // 私人 DJ
  const startPrivateDj = () =>
    runStart('private-dj', async () => {
      const { songs } = await neteaseRadio.privateDj();
      if (!songs.length) throw new Error('私人 DJ 返回为空');
      PlayerStore.setPlaylist(songs);
      PlayerStore.play(songs[0], songs);
      return songs.length;
    });

  // 跑步漫游（bpm 输入）
  const sportBpmOpen = ref(false);
  const sportBpm = ref(150);
  const startSportRun = () =>
    runStart('sport-run', async () => {
      const songs = await neteaseRadio.sportRun(sportBpm.value);
      if (!songs.length) throw new Error('跑步漫游返回为空');
      PlayerStore.setPlaylist(songs);
      PlayerStore.play(songs[0], songs);
      sportBpmOpen.value = false;
      return songs.length;
    });

  // 相似音乐（取当前曲目的网易云 id）
  const currentNeteaseSongId = computed(() => {
    const t = PlayerStore.state.currentTrack;
    const sid = t?.sourceIds?.find((s) => s?.sourceName === 'netease' && s?.entityType === 'Song');
    return sid ? sid.entityId.replace(/^song:/, '') : null;
  });

  const startSimilar = () =>
    runStart('similar', async () => {
      const id = currentNeteaseSongId.value;
      if (!id) throw new Error('当前歌曲没有网易云来源');
      const songs = await neteaseRadio.similarSongs(id, 10);
      if (!songs.length) throw new Error('相似歌曲返回为空');
      PlayerStore.setPlaylist(songs);
      PlayerStore.play(songs[0], songs);
      return songs.length;
    });

  // 最近播放条目播放
  async function playRecent(track) {
    const tracks = recentTracks.value;
    PlayerStore.play(track, tracks);
  }

  // 清空最近播放
  async function handleClearRecent() {
    await clearRecentPlays();
    await loadRecent();
  }

  // 创建歌单
  async function handleCreatePlaylist() {
    const name = window.prompt('新歌单名称：', '我的歌单');
    if (!name || !name.trim()) return;
    try {
      await playlistApi.create(name.trim());
      await loadPlaylists();
      router.push('/playlists');
    } catch (e) {
      showToast(e?.message || '创建失败');
    }
  }

  return {
    sectionOpen,
    neteaseEnabled,
    playlists,
    loadPlaylists,
    recentTracks,
    loadRecent,
    busyItem,
    toast,
    showToast,
    startSmartMix,
    startShuffle,
    startPersonalFm,
    startPrivateDj,
    startSportRun,
    startSimilar,
    sportBpmOpen,
    sportBpm,
    currentNeteaseSongId,
    endlessMode: endless.endlessMode,
    playRecent,
    handleClearRecent,
    handleCreatePlaylist,
  };
}