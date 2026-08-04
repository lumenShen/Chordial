import { ref, shallowRef, computed, watch, onMounted, nextTick } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { library, netease } from '@/api/musicSource';
import { useAnime } from '@/composables/useAnime.js';

/**
 * 搜索页核心组合式逻辑。
 *
 * 职责：
 *  - 持有全部搜索状态（query / 筛选 / 结果 / 加载 / 错误 / 时间）
 *  - 跨来源搜索（本地 / 网易云 / 全部）与按 netease entity_id 去重
 *  - 输入防抖（250ms）与提交立即搜索
 *  - URL query 同步（读取初始值 + replace 回写）
 *  - 加载动画生命周期（anime.js 作用域，跟随页面根节点）
 *
 * @param {() => Element | undefined | null} rootRefGetter 页面根节点 ref 的 getter
 * @returns {object} 页面所需的全部响应式状态与交互方法
 */
export function useSearch(rootRefGetter) {
  const route = useRoute();
  const router = useRouter();

  // ── 状态 ────────────────────────────────────────────────────
  const query = ref('');
  const entityType = ref('all');          // 'all' | 'song' | 'artist' | 'album'
  const sourceName = ref('all');          // 'all' | 'local' | 'netease'
  const limitPerType = ref(50);

  // shallowRef：业务类实例数组，避免深响应式代理开销
  const songs = shallowRef([]);
  const artists = shallowRef([]);
  const albums = shallowRef([]);

  const isLoading = ref(false);
  const hasSearched = ref(false);
  const errorMsg = ref('');
  const searchTimeMs = ref(null);
  // 网易云来源非致命错误（未配置 / 网络失败），不阻塞本地结果展示
  const neteaseWarning = ref('');

  // ── 来源选项 ───────────────────────────────────────────────
  const sourceOptions = [
    { value: 'all', label: '全部来源' },
    { value: 'local', label: '本地' },
    { value: 'netease', label: '网易云音乐' },
  ];

  // ── 类型选项 ────────────────────────────────────────────────
  const typeOptions = [
    { value: 'all', label: '全部' },
    { value: 'song', label: '歌曲' },
    { value: 'artist', label: '歌手' },
    { value: 'album', label: '专辑' },
  ];

  // ── 计数显示 ────────────────────────────────────────────────
  const totalCount = computed(() => songs.value.length + artists.value.length + albums.value.length);
  const hasAnyResult = computed(() => totalCount.value > 0);

  // ── 动画 ────────────────────────────────────────────────────
  const { run } = useAnime(rootRefGetter);

  function playLoadingSpinner() {
    run(({ animate, loopPresets }) => {
      animate('.loading-state .spinner', { ...loopPresets.spin });
    });
  }

  // ── 搜索执行 ────────────────────────────────────────────────
  let debounceTimer = null;

  /**
   * 从实体的 sourceIds 中提取网易云 entity_id（如 "song:123456"）。
   * 用于跨来源去重：本地导入过的网易云歌曲与在线搜索结果视为同一条。
   */
  function neteaseEntityIdOf(entity) {
    if (!entity?.sourceIds) return null;
    for (const sid of entity.sourceIds) {
      if (sid?.sourceName === 'netease' && sid?.entityId) {
        return sid.entityId;
      }
    }
    return null;
  }

  /**
   * 收集已存在列表中所有网易云 entity_id，用于去重过滤。
   */
  function collectNeteaseIds(existing) {
    const set = new Set();
    for (const item of existing) {
      const eid = neteaseEntityIdOf(item);
      if (eid) set.add(eid);
    }
    return set;
  }

  /**
   * 并行调用网易云搜索（歌曲/歌手/专辑），按 entityType 决定调用哪些接口。
   * 任何子调用失败都不会 reject，而是返回部分结果并通过 warning 提示。
   *
   * @param {string} q
   * @param {string|null} et - 'song' | 'artist' | 'album' | null
   * @param {number} limit
   * @returns {Promise<{ songs: any[], artists: any[], albums: any[], warning: string }>}
   */
  async function searchNetease(q, et, limit) {
    const result = { songs: [], artists: [], albums: [], warning: '' };
    const tasks = [];
    if (et === null || et === 'song') {
      tasks.push(
        netease.searchSongs(q).then((s) => { result.songs = s; })
          .catch((e) => { result.warning = e?.message || String(e); }),
      );
    }
    if (et === null || et === 'artist') {
      tasks.push(
        netease.searchArtists(q, Math.min(limit, 30))
          .then((a) => { result.artists = a; })
          .catch((e) => { result.warning = e?.message || String(e); }),
      );
    }
    if (et === null || et === 'album') {
      tasks.push(
        netease.searchAlbums(q, Math.min(limit, 30))
          .then((a) => { result.albums = a; })
          .catch((e) => { result.warning = e?.message || String(e); }),
      );
    }
    await Promise.all(tasks);
    return result;
  }

  async function executeSearch() {
    const q = query.value.trim();
    if (!q) {
      songs.value = [];
      artists.value = [];
      albums.value = [];
      hasSearched.value = false;
      neteaseWarning.value = '';
      return;
    }

    // 取消前一次 debounce
    if (debounceTimer) {
      clearTimeout(debounceTimer);
      debounceTimer = null;
    }

    isLoading.value = true;
    errorMsg.value = '';
    neteaseWarning.value = '';
    hasSearched.value = true;
    nextTick(playLoadingSpinner);

    const t0 = performance.now();
    try {
      const et = entityType.value === 'all' ? null : entityType.value;
      const sn = sourceName.value; // 'all' | 'local' | 'netease'
      const limit = limitPerType.value;

      // ── 仅本地 ─────────────────────────────────
      if (sn === 'local') {
        const data = await library.search({
          query: q,
          entityType: et,
          sourceName: 'local',
          limitPerType: limit,
        });
        songs.value = data.songs;
        artists.value = data.artists;
        albums.value = data.albums;
      }
      // ── 仅网易云 ────────────────────────────────
      else if (sn === 'netease') {
        const data = await searchNetease(q, et, limit);
        songs.value = data.songs;
        artists.value = data.artists;
        albums.value = data.albums;
        neteaseWarning.value = data.warning;
      }
      // ── 全部来源：本地 + 网易云并行，按 netease entity_id 去重 ──
      else {
        const localP = library.search({
          query: q,
          entityType: et,
          sourceName: null,
          limitPerType: limit,
        });
        const neteaseP = searchNetease(q, et, limit);
        const [localData, neteaseData] = await Promise.all([localP, neteaseP]);

        // 收集本地结果中已存在的网易云 entity_id，用于剔除在线重复
        const songIds = collectNeteaseIds(localData.songs);
        const artistIds = collectNeteaseIds(localData.artists);
        const albumIds = collectNeteaseIds(localData.albums);

        const mergedSongs = [
          ...localData.songs,
          ...neteaseData.songs.filter((s) => {
            const eid = neteaseEntityIdOf(s);
            return !eid || !songIds.has(eid);
          }),
        ];
        const mergedArtists = [
          ...localData.artists,
          ...neteaseData.artists.filter((a) => {
            const eid = neteaseEntityIdOf(a);
            return !eid || !artistIds.has(eid);
          }),
        ];
        const mergedAlbums = [
          ...localData.albums,
          ...neteaseData.albums.filter((a) => {
            const eid = neteaseEntityIdOf(a);
            return !eid || !albumIds.has(eid);
          }),
        ];

        songs.value = mergedSongs;
        artists.value = mergedArtists;
        albums.value = mergedAlbums;
        neteaseWarning.value = neteaseData.warning;
      }

      searchTimeMs.value = Math.round(performance.now() - t0);
    } catch (e) {
      console.error('搜索失败:', e);
      errorMsg.value = e?.message || String(e);
      songs.value = [];
      artists.value = [];
      albums.value = [];
    } finally {
      isLoading.value = false;
    }
  }

  function onInput() {
    // debounce 250ms；同时同步 URL query（不触发导航）
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      executeSearch();
    }, 250);
  }

  function onSubmit() {
    if (debounceTimer) {
      clearTimeout(debounceTimer);
      debounceTimer = null;
    }
    executeSearch();
  }

  // 任一筛选条件变更 → 立即重新搜索（无 debounce）
  watch([entityType, sourceName, limitPerType], () => {
    if (hasSearched.value || query.value.trim()) {
      executeSearch();
    }
  });

  // ── URL 同步 ────────────────────────────────────────────────
  // 从 URL 读取初始 query
  onMounted(() => {
    const q = route.query.q;
    if (typeof q === 'string' && q.trim()) {
      query.value = q;
      executeSearch();
    }
  });

  // query 变化时同步到 URL（replace，不污染历史）
  watch(query, (val) => {
    const trimmed = val.trim();
    const currentQ = route.query.q;
    if (trimmed && trimmed !== currentQ) {
      router.replace({ query: { ...route.query, q: trimmed } });
    } else if (!trimmed && currentQ) {
      const { q: _omit, ...rest } = route.query;
      router.replace({ query: rest });
    }
  });

  // ── 事件处理 ────────────────────────────────────────────────
  function clearSearch() {
    query.value = '';
    songs.value = [];
    artists.value = [];
    albums.value = [];
    hasSearched.value = false;
    errorMsg.value = '';
    neteaseWarning.value = '';
    searchTimeMs.value = null;
  }

  return {
    // 状态
    query,
    entityType,
    sourceName,
    limitPerType,
    songs,
    artists,
    albums,
    isLoading,
    hasSearched,
    errorMsg,
    searchTimeMs,
    neteaseWarning,
    // 选项
    sourceOptions,
    typeOptions,
    // 计算
    totalCount,
    hasAnyResult,
    // 交互
    onInput,
    onSubmit,
    clearSearch,
  };
}