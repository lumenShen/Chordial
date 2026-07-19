<script setup>
import { ref, shallowRef, computed, watch, onMounted, nextTick, useTemplateRef } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import TrackList from '@/components/common/TrackList.vue';
import ArtistList from '@/components/common/ArtistList.vue';
import AlbumList from '@/components/common/AlbumList.vue';
import { library, netease } from '@/api/musicSource';
import { useAnime } from '@/composables/useAnime.js';

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
const rootRef = useTemplateRef('root');
const { run } = useAnime(() => rootRef.value);

function playLoadingSpinner() {
  run(({ animate, loopPresets }) => {
    animate('.loading-state .spinner', { ...loopPresets.spin });
  });
}

// 结果区入场（fadeInUp + stagger）
function playResultsEnter() {
  run(({ animate, stagger, presets }) => {
    animate('.result-section', {
      ...presets.fadeInUp,
      delay: stagger(80),
      onComplete: () => {
        rootRef.value?.querySelectorAll('.result-section').forEach((el) => {
          el.style.transform = '';
        });
      },
    });
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
    nextTick(playResultsEnter);
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
</script>

<template>
  <div class="search-page" ref="root">
    <div class="search-header">
      <h1 class="page-title">搜索</h1>

      <!-- 搜索框 -->
      <div class="search-input-row">
        <div class="search-box">
          <i class="bi bi-search search-icon"></i>
          <input
            v-model="query"
            type="text"
            placeholder="搜索歌曲、歌手、专辑..."
            @input="onInput"
            @keyup.enter="onSubmit"
          />
          <button v-if="query" class="clear-btn" title="清空" @click="clearSearch">
            <i class="bi bi-x-lg"></i>
          </button>
        </div>
      </div>

      <!-- 筛选器 -->
      <div class="filter-row">
        <div class="filter-group">
          <span class="filter-label">类型</span>
          <div class="tab-group">
            <button
              v-for="opt in typeOptions"
              :key="opt.value"
              class="tab-btn"
              :class="{ active: entityType === opt.value }"
              @click="entityType = opt.value"
            >
              {{ opt.label }}
            </button>
          </div>
        </div>

        <div class="filter-group">
          <span class="filter-label">来源</span>
          <select v-model="sourceName" class="source-select">
            <option v-for="opt in sourceOptions" :key="opt.value" :value="opt.value">
              {{ opt.label }}
            </option>
          </select>
        </div>

        <div class="filter-group">
          <span class="filter-label">每类上限</span>
          <input
            v-model.number="limitPerType"
            type="number"
            min="1"
            max="500"
            class="limit-input"
          />
        </div>

        <div v-if="searchTimeMs !== null && !isLoading" class="search-meta">
          {{ totalCount }} 条结果 · {{ searchTimeMs }}ms
        </div>
      </div>
    </div>

    <!-- 错误状态 -->
    <div v-if="errorMsg" class="error-state">
      <i class="bi bi-exclamation-triangle"></i>
      <span>搜索失败：{{ errorMsg }}</span>
    </div>

    <!-- 网易云来源非致命警告（未配置 / 网络失败）—— 独立显示，不阻塞结果展示 -->
    <div v-if="!errorMsg && neteaseWarning && !isLoading" class="warning-state">
      <i class="bi bi-exclamation-circle"></i>
      <span>网易云音乐搜索不可用：{{ neteaseWarning }}</span>
    </div>

    <!-- 加载状态 -->
    <div v-if="!errorMsg && isLoading" class="loading-state">
      <div class="spinner"></div>
      <p class="loading-text">搜索中...</p>
    </div>

    <!-- 空状态：尚未搜索 -->
    <div v-else-if="!errorMsg && !isLoading && !hasSearched" class="empty-state hint-state">
      <i class="bi bi-search empty-icon"></i>
      <h3 class="empty-state-title">输入关键词开始搜索</h3>
      <p class="empty-state-desc">支持歌名、歌手名、专辑名的子串匹配（大小写不敏感）</p>
    </div>

    <!-- 空状态：无结果 -->
    <div v-else-if="!errorMsg && !isLoading && hasSearched && !hasAnyResult" class="empty-state">
      <i class="bi bi-emoji-frown empty-icon"></i>
      <h3 class="empty-state-title">未找到 "{{ query }}" 的相关结果</h3>
      <p class="empty-state-desc">试试其他关键词，或调整筛选条件</p>
    </div>

    <!-- 结果区 -->
    <template v-else-if="!errorMsg && !isLoading && hasSearched && hasAnyResult">
      <!-- 歌曲 -->
      <section v-if="songs.length > 0" class="result-section">
        <h2 class="section-title">
          歌曲 <span class="count-badge">{{ songs.length }}</span>
        </h2>
        <TrackList :tracks="songs" :virtual-scroll="false" />
      </section>

      <!-- 歌手 -->
      <section v-if="artists.length > 0" class="result-section">
        <h2 class="section-title">
          歌手 <span class="count-badge">{{ artists.length }}</span>
        </h2>
        <ArtistList :artists="artists" :virtual-scroll="false" />
      </section>

      <!-- 专辑 -->
      <section v-if="albums.length > 0" class="result-section">
        <h2 class="section-title">
          专辑 <span class="count-badge">{{ albums.length }}</span>
        </h2>
        <AlbumList :albums="albums" />
      </section>
    </template>
  </div>
</template>

<style scoped>
.search-page {
  max-width: 1200px;
  margin: 0 auto;
  padding-bottom: 32px;
}

.search-header {
  margin-bottom: 24px;
}

.page-title {
  font-size: 26px;
  font-weight: 900;
  margin: 0 0 16px 0;
}

/* ── 搜索框 ── */
.search-input-row {
  margin-bottom: 16px;
}

.search-box {
  position: relative;
  display: flex;
  align-items: center;
  max-width: 640px;
}

.search-icon {
  position: absolute;
  left: 14px;
  font-size: 16px;
  color: var(--text-tertiary);
  pointer-events: none;
}

.search-box input {
  width: 100%;
  height: 44px;
  padding: 0 44px 0 44px;
  border: 1px solid var(--border-light);
  border-radius: 22px;
  background: var(--bg-glass);
  color: var(--text-primary);
  font-size: 15px;
  font-weight: 400;
  transition: all var(--transition-normal);
  backdrop-filter: saturate(180%) blur(12px);
  -webkit-backdrop-filter: saturate(180%) blur(12px);
}

.search-box input:focus {
  outline: none;
  border-color: var(--primary-color);
  background: var(--bg-secondary);
  box-shadow: 0 0 0 4px var(--primary-light);
}

.search-box input::placeholder {
  color: var(--text-tertiary);
}

.clear-btn {
  position: absolute;
  right: 10px;
  width: 28px;
  height: 28px;
  border: none;
  border-radius: 50%;
  background: var(--bg-hover);
  color: var(--text-secondary);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 11px;
  transition: all var(--transition-fast);
}

.clear-btn:hover {
  background: var(--bg-active);
  color: var(--text-primary);
}

/* ── 筛选器 ── */
.filter-row {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 16px 20px;
}

.filter-group {
  display: flex;
  align-items: center;
  gap: 8px;
}

.filter-label {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-secondary);
}

.tab-group {
  display: flex;
  background: var(--bg-glass);
  border: 1px solid var(--border-light);
  border-radius: 10px;
  padding: 2px;
  backdrop-filter: saturate(180%) blur(12px);
  -webkit-backdrop-filter: saturate(180%) blur(12px);
}

.tab-btn {
  padding: 6px 14px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  font-size: 13px;
  font-weight: 500;
  border-radius: 8px;
  cursor: pointer;
  transition: all var(--transition-fast);
}

.tab-btn:hover {
  color: var(--text-primary);
}

.tab-btn.active {
  background: var(--primary-color);
  color: #fff;
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.15);
}

.source-select,
.limit-input {
  height: 32px;
  padding: 0 10px;
  border: 1px solid var(--border-light);
  border-radius: 8px;
  background: var(--bg-glass);
  color: var(--text-primary);
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: all var(--transition-fast);
}

.source-select:focus,
.limit-input:focus {
  outline: none;
  border-color: var(--primary-color);
  box-shadow: 0 0 0 3px var(--primary-light);
}

.limit-input {
  width: 80px;
  cursor: text;
}

.search-meta {
  margin-left: auto;
  font-size: 12px;
  color: var(--text-tertiary);
  font-variant-numeric: tabular-nums;
}

/* ── 状态 ── */
.loading-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 60px 20px;
  gap: 16px;
}

.loading-state .spinner {
  animation: none; /* 由 anime.js 驱动 */
  width: 32px;
  height: 32px;
  border: 3px solid var(--border-light);
  border-top-color: var(--primary-color);
  border-radius: 50%;
}

.loading-text {
  font-size: 14px;
  color: var(--text-secondary);
  margin: 0;
}

.error-state {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 16px 20px;
  border-radius: 12px;
  background: rgba(255, 99, 71, 0.1);
  color: #ff6347;
  font-size: 14px;
}

.error-state i {
  font-size: 18px;
}

.warning-state {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 18px;
  margin-bottom: 16px;
  border-radius: 12px;
  background: rgba(255, 193, 7, 0.12);
  color: #c98a00;
  font-size: 13px;
}

.warning-state i {
  font-size: 16px;
}

@media (prefers-color-scheme: dark) {
  .warning-state {
    background: rgba(255, 193, 7, 0.08);
    color: #ffcf4d;
  }
}

.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 80px 20px;
  text-align: center;
  gap: 8px;
}

.empty-state.hint-state {
  padding: 100px 20px;
}

.empty-icon {
  font-size: 56px;
  color: var(--text-tertiary);
  margin-bottom: 8px;
  opacity: 0.5;
}

.empty-state-title {
  font-size: 18px;
  font-weight: 700;
  color: var(--text-primary);
  margin: 0;
}

.empty-state-desc {
  font-size: 13px;
  color: var(--text-tertiary);
  margin: 0;
}

/* ── 结果区 ── */
.result-section {
  margin-top: 32px;
}

.section-title {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 18px;
  font-weight: 700;
  color: var(--text-primary);
  margin: 0 0 16px 0;
}

.count-badge {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 24px;
  height: 22px;
  padding: 0 8px;
  border-radius: 11px;
  background: var(--bg-hover);
  color: var(--text-secondary);
  font-size: 12px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}

/* ── 响应式 ── */
@media (max-width: 768px) {
  .search-page {
    padding: 0 12px 32px;
  }

  .page-title {
    font-size: 22px;
  }

  .filter-row {
    gap: 12px;
  }

  .search-meta {
    margin-left: 0;
    width: 100%;
    text-align: right;
  }

  .empty-icon {
    font-size: 44px;
  }
}

@media (max-width: 480px) {
  .filter-group {
    width: 100%;
    justify-content: space-between;
  }

  .tab-group {
    flex: 1;
  }

  .tab-btn {
    flex: 1;
    padding: 6px 8px;
    font-size: 12px;
  }
}

@media (prefers-color-scheme: dark) {
  .search-box input {
    background: var(--bg-glass);
    border-color: var(--border-light);
  }

  .loading-state .spinner {
    border-color: rgba(255, 255, 255, 0.15);
    border-top-color: var(--primary-color);
  }
}
</style>
