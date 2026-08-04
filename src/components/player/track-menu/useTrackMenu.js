/**
 * useTrackMenu — AMLL MenuButton 触发的歌曲菜单 Sheet 的组合式逻辑
 *
 * 由 TrackMenuSheet.vue 及其子组件（track-menu/*）共享。
 * 所有响应式状态均为模块级单例，任何组件调用 useTrackMenu() 拿到的都是同一份状态，
 * 从而在拆分后保持对外行为完全一致（可见性 / track watch / toast / 各 API 调用 / source 切换）。
 *
 * 六大功能区：
 *   1. 歌曲操作：添加到歌单 / 下载 / 跳转详情
 *   2. 跳转：艺术家 / 专辑
 *   3. 来源匹配：搜索网易云同名曲，预览/切换播放
 *   4. 来源切换：当前曲目的所有 sourceIds，选择后重排 sourceIds[0] 并重播
 *   5. 音质信息：基于文件扩展名推断 codec / 等级 / 码率
 *   6. 打开：本地目录（revealItemInDir）/ 在线页面（openUrl）
 */
import { ref, computed, shallowRef, watch } from 'vue';
import { useRouter } from 'vue-router';
import { PlayerStore } from '@/stores/player/index.js';
import * as playlistApi from '@/api/musicSource/playlists.js';
import * as neteaseApi from '@/api/musicSource/netease.js';
import { platformIsTauri } from '@/composables/usePlatform.js';

// ── Sheet 可见性 ────────────────────────────────────────────
const visible = computed({
  get: () => PlayerStore.state.ui.isTrackMenuOpen,
  set: (v) => (v ? PlayerStore.openTrackMenu() : PlayerStore.closeTrackMenu()),
});

// ── 当前播放曲目（shallowRef 避免 Vue 深代理业务类实例）─────
const track = shallowRef(null);
// ↓ 提前声明：watch immediate 回调中要重置这些状态，必须先于 watch 初始化
//   （const ref 存在 TDZ，否则会抛 "Cannot access 'X' before initialization"）
const onlineMatches = ref([]);
const onlineSearching = ref(false);
const onlineSearchDone = ref(false);
const playlistPickerOpen = ref(false);

// ── Toast（轻量级提示）──────────────────────────────────────
const toast = ref(null);
let toastTimer = null;
function showToast(msg, type = 'info', duration = 2200) {
  toast.value = { msg, type };
  if (toastTimer) clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (toast.value = null), duration);
}

// ════════════════════════════════════════════════════════════
// Section 1: 歌曲操作
// ════════════════════════════════════════════════════════════

const playlists = ref([]);
let playlistsLoaded = false;

async function loadPlaylists() {
  if (playlistsLoaded) return;
  try {
    const list = await playlistApi.list();
    playlists.value = list || [];
    playlistsLoaded = true;
  } catch (e) {
    console.warn('[TrackMenuSheet] 加载歌单失败:', e);
  }
}

async function togglePlaylistPicker() {
  playlistPickerOpen.value = !playlistPickerOpen.value;
  if (playlistPickerOpen.value) await loadPlaylists();
}

async function addToPlaylist(playlistId) {
  if (!track.value?.id) return;
  try {
    await playlistApi.addSong(playlistId, track.value.id);
    showToast('已添加到歌单', 'success');
  } catch (e) {
    showToast(`添加失败：${e.message || e}`, 'error');
  } finally {
    playlistPickerOpen.value = false;
  }
}

async function createPlaylistAndAdd() {
  if (!track.value?.id) return;
  const name = window.prompt('请输入新歌单名称', '我的收藏');
  if (!name || !name.trim()) return;
  try {
    const pl = await playlistApi.create(name.trim(), null);
    await playlistApi.addSong(pl.id, track.value.id);
    playlistsLoaded = false;
    await loadPlaylists();
    showToast('已创建歌单并添加', 'success');
  } catch (e) {
    showToast(`操作失败：${e.message || e}`, 'error');
  } finally {
    playlistPickerOpen.value = false;
  }
}

function viewTrackDetail() {
  if (!track.value?.id) return;
  PlayerStore.closeTrackMenu();
  PlayerStore.closePlayerView();
  router.push(`/track/${track.value.id}`);
}

// ════════════════════════════════════════════════════════════
// Section 2: 跳转 — 艺术家 / 专辑
// ════════════════════════════════════════════════════════════

function gotoArtist() {
  const t = track.value;
  if (!t) return;
  const artistId = t.artistIds?.[0];
  if (!artistId) {
    showToast('该曲目未关联艺术家', 'info');
    return;
  }
  PlayerStore.closeTrackMenu();
  PlayerStore.closePlayerView();
  router.push(`/artists/${artistId}`);
}

function gotoAlbum() {
  const t = track.value;
  if (!t) return;
  if (!t.albumId) {
    showToast('该曲目未关联专辑', 'info');
    return;
  }
  PlayerStore.closeTrackMenu();
  PlayerStore.closePlayerView();
  router.push(`/albums/${t.albumId}`);
}

// ════════════════════════════════════════════════════════════
// Section 3: 来源匹配 — 搜索网易云同名曲
// ════════════════════════════════════════════════════════════

async function searchOnlineMatches() {
  const t = track.value;
  if (!t) return;
  onlineSearching.value = true;
  onlineSearchDone.value = false;
  onlineMatches.value = [];
  try {
    const query = `${t.title || ''} ${t.artistNames?.[0] || ''}`.trim();
    if (!query) {
      showToast('无有效关键词', 'info');
      return;
    }
    // 仅 netease 提供 searchSongs 接口
    const results = await neteaseApi.searchSongs(query);
    onlineMatches.value = (results || []).slice(0, 20);
  } catch (e) {
    showToast(`搜索失败：${e.message || e}`, 'error');
  } finally {
    onlineSearching.value = false;
    onlineSearchDone.value = true;
  }
}

/**
 * 用搜索到的网易云曲目创建新 Song 实例并播放（不影响库内当前曲目）。
 * 选择"在线匹配"的曲目后，相当于切换到一个未入库的纯在线版本。
 */
function playOnlineMatch(song) {
  if (!song) return;
  PlayerStore.closeTrackMenu();
  // 以单曲列表播放，避免污染原播放列表
  PlayerStore.play(song, [song]);
  showToast(`切换到网易云版本：${song.title}`, 'success');
}

// ════════════════════════════════════════════════════════════
// Section 4: 来源切换 — 当前曲目的所有 sourceIds
// ════════════════════════════════════════════════════════════

/** 当前曲目所有 SourceId；sourceIds[0] 即当前播放使用的来源 */
const sourceIds = computed(() => track.value?.sourceIds ?? []);

/** 当前激活的来源（即 getAudioBlobUrl 使用的 sourceIds[0]） */
const activeSourceId = computed(() => sourceIds.value[0] ?? null);

const switching = ref(false);

/**
 * 切换播放来源：将选中的 SourceId 移到 sourceIds[0]，然后重播。
 *
 * 实现细节：
 * - Song.getAudioBlobUrl() 始终用 sourceIds[0]；重排后调用 PlayerStore.play(track)
 *   会重新加载音频源。
 * - 直接原地修改 track.sourceIds（markRaw 实例不被深代理，安全）。
 */
async function switchSource(sid) {
  const t = track.value;
  if (!t || !sid) return;
  if (sid === activeSourceId.value) return; // 已是当前来源
  switching.value = true;
  try {
    const idx = t.sourceIds.indexOf(sid);
    if (idx <= 0) return;
    // 重排：选中项移到首位，其余保持顺序
    const [removed] = t.sourceIds.splice(idx, 1);
    t.sourceIds.unshift(removed);
    // 重新播放同一首 track（getAudioBlobUrl 会取新的 sourceIds[0]）
    await PlayerStore.play(t, PlayerStore.state.playlist);
    showToast('已切换播放来源', 'success');
  } catch (e) {
    showToast(`切换失败：${e.message || e}`, 'error');
  } finally {
    switching.value = false;
  }
}

/** 来源显示名映射 */
function sourceDisplayName(sid) {
  if (!sid) return '未知';
  const map = { local: '本地文件', netease: '网易云音乐' };
  return map[sid.sourceName] || sid.sourceName;
}

/** 来源实体 ID 短显示（仅显示后 12 字符）*/
function sourceEntityShort(sid) {
  if (!sid?.entityId) return '';
  const eid = sid.entityId;
  return eid.length > 24 ? `…${eid.slice(-12)}` : eid;
}

// ════════════════════════════════════════════════════════════
// Section 5: 音质信息 — 基于文件扩展名推断
// ════════════════════════════════════════════════════════════

/** 从 SourceId.entityId 提取文件扩展名（本地来源 entity_id 为文件路径）*/
function extFromEntity(sid) {
  if (!sid?.entityId) return '';
  const dot = sid.entityId.lastIndexOf('.');
  if (dot < 0) return '';
  return sid.entityId.slice(dot + 1).toLowerCase();
}

/** 音频 codec 推断 */
function codecFromExt(ext) {
  const m = {
    flac: 'FLAC',
    wav: 'WAV',
    mp3: 'MP3',
    m4a: 'AAC',
    aac: 'AAC',
    ogg: 'Vorbis',
    opus: 'Opus',
    ape: 'APE',
    alac: 'ALAC',
  };
  return m[ext] || (ext ? ext.toUpperCase() : '未知');
}

/** 音质等级推断（与后端 AudioQualityDescriptor::from_extension 一致）*/
function qualityTierFromExt(ext) {
  const m = {
    flac: '无损',
    wav: '无损',
    alac: '无损',
    ape: '无损',
    m4a: '较高',
    aac: '较高',
    mp3: '有损',
    ogg: '有损',
    opus: '有损',
  };
  return m[ext] || '未知';
}

const currentQuality = computed(() => {
  const sid = activeSourceId.value;
  if (!sid) return null;
  const ext = extFromEntity(sid);
  return {
    ext,
    codec: codecFromExt(ext),
    tier: qualityTierFromExt(ext),
    isLocal: sid.isLocal(),
    sourceName: sid.sourceName,
  };
});

// ════════════════════════════════════════════════════════════
// Section 6: 打开本地目录 / 在线页面
// ════════════════════════════════════════════════════════════

const isTauri = platformIsTauri();

/** 本地来源：在文件管理器中显示文件位置 */
async function revealInFolder() {
  const sid = activeSourceId.value;
  if (!sid?.isLocal() || !sid.entityId) {
    showToast('当前来源不支持本地目录打开', 'info');
    return;
  }
  if (!isTauri) {
    showToast('仅桌面端 Tauri 环境支持', 'info');
    return;
  }
  try {
    const { revealItemInDir } = await import('@tauri-apps/plugin-opener');
    await revealItemInDir(sid.entityId);
  } catch (e) {
    showToast(`打开失败：${e.message || e}`, 'error');
  }
}

/** 网易云来源：打开网易云歌曲网页 */
async function openOnlinePage() {
  const sid = activeSourceId.value;
  if (!sid) return;
  // 网易云 entity_id 格式：song:<remote_id> 或 song:<remote_id>:<title>
  let url = null;
  if (sid.sourceName === 'netease') {
    const parts = sid.entityId.split(':');
    const remoteId = parts[1];
    if (remoteId) url = `https://music.163.com/#/song?id=${remoteId}`;
  }
  if (!url) {
    showToast('当前来源无在线页面', 'info');
    return;
  }
  if (isTauri) {
    try {
      const { openUrl } = await import('@tauri-apps/plugin-opener');
      await openUrl(url);
    } catch (e) {
      showToast(`打开失败：${e.message || e}`, 'error');
    }
  } else {
    window.open(url, '_blank', 'noopener');
  }
}

/** 是否能打开本地目录（本地来源 + Tauri 环境）*/
const canRevealInFolder = computed(() => {
  const sid = activeSourceId.value;
  return isTauri && sid?.isLocal() && !!sid.entityId;
});

/** 是否有可访问的在线页面 */
const canOpenOnline = computed(() => {
  const sid = activeSourceId.value;
  if (!sid) return false;
  if (sid.sourceName === 'netease') {
    return !!sid.entityId.split(':')[1];
  }
  return false;
});

// ── 关闭时清理 ─────────────────────────────────────────────
function onClose() {
  playlistPickerOpen.value = false;
}

// ── 初始化（组件首次调用时执行一次）────────────────────────
let router = null;
let wired = false;
function init() {
  if (!router) router = useRouter();
  if (!wired) {
    wired = true;
    watch(
      () => PlayerStore.state.currentTrack,
      (t) => {
        track.value = t ?? null;
        // 切换曲目时重置即时状态
        onlineMatches.value = [];
        onlineSearchDone.value = false;
        onlineSearching.value = false;
        playlistPickerOpen.value = false;
      },
      { immediate: true },
    );
  }
}

/**
 * 组合式入口：返回共享状态与操作函数。
 * 必须在组件 setup 内调用（内部使用 useRouter）。
 */
export function useTrackMenu() {
  init();
  return {
    // 可见性 / 曲目
    visible,
    track,
    // Toast
    toast,
    showToast,
    // 歌曲操作
    playlists,
    playlistPickerOpen,
    loadPlaylists,
    togglePlaylistPicker,
    addToPlaylist,
    createPlaylistAndAdd,
    viewTrackDetail,
    // 跳转
    gotoArtist,
    gotoAlbum,
    // 来源匹配
    onlineMatches,
    onlineSearching,
    onlineSearchDone,
    searchOnlineMatches,
    playOnlineMatch,
    // 来源切换
    sourceIds,
    activeSourceId,
    switching,
    switchSource,
    sourceDisplayName,
    sourceEntityShort,
    // 音质
    currentQuality,
    // 打开
    isTauri,
    canRevealInFolder,
    canOpenOnline,
    revealInFolder,
    openOnlinePage,
    // 关闭清理
    onClose,
  };
}