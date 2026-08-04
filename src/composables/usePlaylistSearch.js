/**
 * usePlaylistSearch — PlaylistSearchView 在线搜索网易云歌单的逻辑 Composable
 *
 * 流程：
 *  1. 用户输入关键词，调用 netease_search_playlists 拉取在线歌单列表
 *  2. 点击歌单卡片预览详情（曲目数 / 创建者 / 描述）
 *  3. "保存到本地" 按钮 → netease_save_playlist_to_library
 *     后端会自动把歌单中每首歌加入音乐库（去重），并创建本地 Playlist 记录
 */
import { ref, shallowRef, computed } from 'vue';
import { useRouter } from 'vue-router';
import * as neteaseApi from '@/api/musicSource/netease.js';

export function usePlaylistSearch() {
  const router = useRouter();

  const query = ref('');
  const limit = ref(30);
  const offset = ref(0);

  // shallowRef：业务对象数组，避免深代理
  const results = shallowRef([]);
  const totalCount = ref(0);
  const isLoading = ref(false);
  const errorMsg = ref('');

  // 预览弹窗
  const previewDetail = ref(null);
  const previewLoading = ref(false);
  const previewTracks = shallowRef([]);
  const saving = ref(false);

  // 已保存的歌单 ID（本地去重提示）
  const savedIds = ref(new Set());

  // Toast
  const toast = ref(null);
  let toastTimer = null;
  function showToast(msg, type = 'info', duration = 2500) {
    toast.value = { msg, type };
    if (toastTimer) clearTimeout(toastTimer);
    toastTimer = setTimeout(() => { toast.value = null; }, duration);
  }

  // ── 搜索 ──────────────────────────────────────────────
  let debounceTimer = null;

  async function executeSearch() {
    const q = query.value.trim();
    if (!q) return;
    isLoading.value = true;
    errorMsg.value = '';
    try {
      const list = await neteaseApi.searchPlaylists(q, limit.value, offset.value);
      results.value = list || [];
      totalCount.value = (list || []).length;
    } catch (e) {
      errorMsg.value = e.message || String(e);
      results.value = [];
    } finally {
      isLoading.value = false;
    }
  }

  function onInput() {
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(executeSearch, 350);
  }

  function onSubmit() {
    if (debounceTimer) {
      clearTimeout(debounceTimer);
      debounceTimer = null;
    }
    executeSearch();
  }

  function clearSearch() {
    query.value = '';
    results.value = [];
    totalCount.value = 0;
    errorMsg.value = '';
  }

  // ── 预览 ──────────────────────────────────────────────
  async function previewPlaylist(item) {
    previewLoading.value = true;
    previewDetail.value = item;
    previewTracks.value = [];
    try {
      const data = await neteaseApi.getPlaylistFull(item.id);
      if (data) {
        previewDetail.value = {
          ...item,
          ...(data.detail || {}),
        };
        previewTracks.value = data.tracks || [];
      }
    } catch (e) {
      showToast(`加载详情失败：${e.message || e}`, 'error');
    } finally {
      previewLoading.value = false;
    }
  }

  function closePreview() {
    previewDetail.value = null;
    previewTracks.value = [];
  }

  // ── 保存到本地 ────────────────────────────────────────
  async function saveToLocal() {
    if (!previewDetail.value) return;
    saving.value = true;
    try {
      const playlist = await neteaseApi.savePlaylistToLibrary(previewDetail.value.id);
      savedIds.value.add(previewDetail.value.id);
      showToast(`已保存到本地：${playlist.name || previewDetail.value.name}`, 'success');
      closePreview();
    } catch (e) {
      showToast(`保存失败：${e.message || e}`, 'error');
    } finally {
      saving.value = false;
    }
  }

  // ── 跳转本地歌单 ─────────────────────────────────────
  function goLocalPlaylists() {
    router.push('/playlists');
  }

  // ── 工具 ─────────────────────────────────────────────
  function formatPlayCount(n) {
    if (n == null) return '';
    if (n >= 1e8) return (n / 1e8).toFixed(1) + '亿';
    if (n >= 1e4) return (n / 1e4).toFixed(1) + '万';
    return String(n);
  }

  const hasResults = computed(() => results.value.length > 0);

  return {
    query,
    limit,
    offset,
    results,
    totalCount,
    isLoading,
    errorMsg,
    previewDetail,
    previewLoading,
    previewTracks,
    saving,
    savedIds,
    toast,
    hasResults,
    executeSearch,
    onInput,
    onSubmit,
    clearSearch,
    previewPlaylist,
    closePreview,
    saveToLocal,
    goLocalPlaylists,
    formatPlayCount,
    showToast,
  };
}

export default usePlaylistSearch;