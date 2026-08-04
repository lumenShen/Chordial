<script setup>
/**
 * TrackMenuSheet — AMLL MenuButton 触发的歌曲菜单 Sheet
 *
 * 由 PlayerView.vue 内挂载，监听 PlayerStore.state.ui.isTrackMenuOpen 显示。
 * 触发链路：AMLL musicInfo MenuButton → onRequestOpenMenuAtom.onEmit
 *        → PlayerStore.openTrackMenu() → isTrackMenuOpen=true → 本组件 BottomSheet 显示
 *
 * 六大功能区：
 *   1. 歌曲操作：添加到歌单 / 下载 / 跳转详情
 *   2. 跳转：艺术家 / 专辑
 *   3. 来源匹配：搜索网易云同名曲，预览/切换播放
 *   4. 来源切换：当前曲目的所有 sourceIds，选择后重排 sourceIds[0] 并重播
 *   5. 音质信息：基于文件扩展名推断 codec / 等级 / 码率
 *   6. 打开：本地目录（revealItemInDir）/ 在线页面（openUrl）
 */
import { ref, computed, watch, shallowRef } from 'vue';
import { useRouter } from 'vue-router';
import BottomSheet from '@/components/ui/BottomSheet.vue';
import { PlayerStore } from '@/stores/player/index.js';
import { useCoverImage } from '@/composables/useCoverImage';
import * as playlistApi from '@/api/musicSource/playlists.js';
import * as neteaseApi from '@/api/musicSource/netease.js';
import * as sourceApi from '@/api/musicSource/sources.js';
import { platformIsTauri } from '@/composables/usePlatform.js';
import { Song } from '@/class';

const router = useRouter();

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

// ── 封面（小尺寸缩略图，复用 composable 缓存）─────────────
const { coverUrl } = useCoverImage(track, 'small');

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
</script>

<template>
  <BottomSheet
    v-model:visible="visible"
    title="歌曲菜单"
    :detents="['large']"
    default-detent="large"
    @close="onClose"
  >
    <div v-if="!track" class="tms-empty">未在播放任何曲目</div>

    <div v-else class="tms-root">
      <!-- ── 顶部曲目卡片 ─────────────────────────────── -->
      <header class="tms-hero">
        <div class="tms-cover">
          <img v-if="coverUrl" :src="coverUrl" :alt="track.title" />
            <HIcon v-else name="music-note-beamed" class="tms-cover-placeholder" />
        </div>
        <div class="tms-meta">
          <div class="tms-title" :title="track.title">{{ track.title || '未知歌曲' }}</div>
          <div class="tms-artist" :title="track.artist">
            {{ track.artist || '未知歌手' }}
          </div>
          <div v-if="track.albumTitle" class="tms-album">
              <HIcon name="disc" />
            <span :title="track.albumTitle">{{ track.albumTitle }}</span>
          </div>
        </div>
      </header>

      <!-- ════════ Section 1: 歌曲操作 ════════ -->
      <section class="tms-section">
        <h4 class="tms-section-title">歌曲操作</h4>
        <div class="tms-actions">
          <button class="tms-action-btn" @click="togglePlaylistPicker">
              <HIcon name="plus-square" />
            <span>添加到歌单</span>
          </button>
          <button class="tms-action-btn" @click="viewTrackDetail">
              <HIcon name="info-circle" />
            <span>查看详情</span>
          </button>
        </div>

        <!-- 歌单选择子列表 -->
        <div v-if="playlistPickerOpen" class="tms-sublist">
          <div v-if="playlists.length === 0" class="tms-sublist-empty">
            暂无歌单
          </div>
          <button
            v-for="pl in playlists"
            :key="pl.id"
            class="tms-sublist-item"
            @click="addToPlaylist(pl.id)"
          >
              <HIcon name="music-note-list" />
            <span class="tms-sublist-name">{{ pl.name }}</span>
            <span class="tms-sublist-count">{{ pl.song_ids?.length || 0 }} 首</span>
          </button>
          <button class="tms-sublist-item tms-sublist-create" @click="createPlaylistAndAdd">
              <HIcon name="plus-circle" />
            <span>创建新歌单</span>
          </button>
        </div>
      </section>

      <!-- ════════ Section 2: 跳转 ════════ -->
      <section class="tms-section">
        <h4 class="tms-section-title">跳转</h4>
        <div class="tms-actions">
          <button
            class="tms-action-btn"
            :disabled="!track.artistIds?.length"
            @click="gotoArtist"
          >
              <HIcon name="person" />
            <span>查看艺术家</span>
          </button>
          <button
            class="tms-action-btn"
            :disabled="!track.albumId"
            @click="gotoAlbum"
          >
              <HIcon name="disc3" />
            <span>查看专辑</span>
          </button>
        </div>
      </section>

      <!-- ════════ Section 3: 来源匹配 ════════ -->
      <section class="tms-section">
        <div class="tms-section-header">
          <h4 class="tms-section-title">来源匹配</h4>
          <button
            class="tms-refresh-btn"
            :disabled="onlineSearching"
            @click="searchOnlineMatches"
          >
              <HIcon name="arrow-repeat" :class="{ 'tms-spin': onlineSearching }" />
            {{ onlineSearchDone ? '重新搜索' : '搜索网易云' }}
          </button>
        </div>

        <div v-if="onlineSearching" class="tms-loading">
            <HIcon name="arrow-repeat" class="tms-spin" /> 搜索中...
        </div>

        <div v-else-if="onlineSearchDone && onlineMatches.length === 0" class="tms-empty-inline">
          未找到匹配曲目
        </div>

        <div v-else-if="onlineMatches.length > 0" class="tms-match-list">
          <button
            v-for="m in onlineMatches"
            :key="m.sourceIds?.find((s) => s.sourceName === 'netease')?.entityId || m.id"
            class="tms-match-item"
            @click="playOnlineMatch(m)"
          >
            <div class="tms-match-info">
              <div class="tms-match-title" :title="m.title">{{ m.title }}</div>
              <div class="tms-match-sub">
                {{ m.artist || '未知歌手' }}
                <span v-if="m.albumTitle"> · {{ m.albumTitle }}</span>
              </div>
            </div>
              <HIcon name="play-circle" class="tms-match-play" />
          </button>
        </div>

        <div v-else class="tms-empty-inline">
          点击"搜索网易云"查找同名曲
        </div>
      </section>

      <!-- ════════ Section 4: 来源切换 ════════ -->
      <section class="tms-section">
        <h4 class="tms-section-title">
          播放来源
          <span class="tms-section-hint">（点击切换，当前来源排在最前）</span>
        </h4>

        <div v-if="sourceIds.length === 0" class="tms-empty-inline">
          该曲目无来源信息
        </div>

        <div v-else class="tms-source-list">
          <button
            v-for="(sid, idx) in sourceIds"
            :key="idx"
            class="tms-source-item"
            :class="{ 'tms-source-active': idx === 0 }"
            :disabled="idx === 0 || switching"
            @click="switchSource(sid)"
          >
            <div class="tms-source-main">
              <i
                class="bi"
                :class="sid.isLocal() ? 'bi-folder' : 'bi-cloud'"
              />
              <div class="tms-source-text">
                <div class="tms-source-name">{{ sourceDisplayName(sid) }}</div>
                <div class="tms-source-id" :title="sid.entityId">
                  {{ sourceEntityShort(sid) }}
                </div>
              </div>
            </div>
              <HIcon v-if="idx === 0" name="check-circle-fill" class="tms-source-check" />
          </button>
        </div>
      </section>

      <!-- ════════ Section 5: 音质信息 ════════ -->
      <section v-if="currentQuality" class="tms-section">
        <h4 class="tms-section-title">音质信息</h4>
        <div class="tms-quality">
          <div class="tms-quality-item">
            <span class="tms-quality-label">来源</span>
            <span class="tms-quality-value">{{ currentQuality.sourceName }}</span>
          </div>
          <div class="tms-quality-item">
            <span class="tms-quality-label">编码</span>
            <span class="tms-quality-value">{{ currentQuality.codec }}</span>
          </div>
          <div class="tms-quality-item">
            <span class="tms-quality-label">等级</span>
            <span class="tms-quality-value">{{ currentQuality.tier }}</span>
          </div>
          <div v-if="currentQuality.ext" class="tms-quality-item">
            <span class="tms-quality-label">扩展名</span>
            <span class="tms-quality-value">.{{ currentQuality.ext }}</span>
          </div>
          <div class="tms-quality-item">
            <span class="tms-quality-label">类型</span>
            <span class="tms-quality-value">
              {{ currentQuality.isLocal ? '本地文件' : '在线流' }}
            </span>
          </div>
        </div>
      </section>

      <!-- ════════ Section 6: 打开 ════════ -->
      <section class="tms-section">
        <h4 class="tms-section-title">打开</h4>
        <div class="tms-actions">
          <button
            class="tms-action-btn"
            :disabled="!canRevealInFolder"
            @click="revealInFolder"
          >
              <HIcon name="folder2-open" />
            <span>在文件管理器中显示</span>
          </button>
          <button
            class="tms-action-btn"
            :disabled="!canOpenOnline"
            @click="openOnlinePage"
          >
              <HIcon name="box-arrow-up-right" />
            <span>打开在线页面</span>
          </button>
        </div>
      </section>
    </div>

    <!-- ── Toast ────────────────────────────────────── -->
    <Transition name="tms-toast">
      <div v-if="toast" class="tms-toast" :class="`tms-toast--${toast.type}`">
        {{ toast.msg }}
      </div>
    </Transition>
  </BottomSheet>
</template>

<style scoped>
.tms-root {
  display: flex;
  flex-direction: column;
  gap: var(--space-4, 16px);
  padding-bottom: var(--space-3, 12px);
}

.tms-empty {
  padding: var(--space-6, 32px) var(--space-4, 16px);
  text-align: center;
  color: var(--text-tertiary, #999);
  font-size: var(--font-size-sm, 13px);
}

/* ── Hero ──────────────────────────────────────── */
.tms-hero {
  display: flex;
  gap: var(--space-3, 12px);
  align-items: center;
  padding: var(--space-3, 12px) 0;
}

.tms-cover {
  width: 64px;
  height: 64px;
  border-radius: var(--radius-md, 10px);
  overflow: hidden;
  background: var(--bg-tertiary, rgba(15, 15, 15, 0.05));
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.tms-cover img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.tms-cover-placeholder {
  font-size: 28px;
  color: var(--text-tertiary, #999);
}

.tms-meta {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.tms-title {
  font-size: var(--font-size-md, 15px);
  font-weight: var(--font-bold, 600);
  color: var(--text-primary, #333);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tms-artist {
  font-size: var(--font-size-sm, 13px);
  color: var(--text-secondary, #666);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tms-album {
  font-size: var(--font-size-sm, 13px);
  color: var(--text-tertiary, #999);
  display: flex;
  align-items: center;
  gap: 4px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* ── Section ───────────────────────────────────── */
.tms-section {
  display: flex;
  flex-direction: column;
  gap: var(--space-2, 8px);
  padding: var(--space-2, 8px) 0;
  border-top: 1px solid var(--bg-tertiary, rgba(15, 15, 15, 0.05));
}

.tms-section:first-of-type {
  border-top: none;
}

.tms-section-title {
  font-size: var(--font-size-xs, 12px);
  font-weight: var(--font-bold, 600);
  color: var(--text-secondary, #666);
  text-transform: uppercase;
  letter-spacing: 0.5px;
  margin: 0;
}

.tms-section-hint {
  font-size: var(--font-size-xs, 11px);
  font-weight: normal;
  color: var(--text-tertiary, #999);
  text-transform: none;
  letter-spacing: 0;
  margin-left: 6px;
}

.tms-section-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.tms-refresh-btn {
  border: none;
  background: var(--bg-tertiary, rgba(15, 15, 15, 0.05));
  color: var(--text-secondary, #666);
  font-size: var(--font-size-xs, 12px);
  padding: 4px 10px;
  border-radius: var(--radius-full, 999px);
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  gap: 4px;
}

.tms-refresh-btn:hover:not(:disabled) {
  background: var(--bg-hover, rgba(15, 15, 15, 0.08));
}

.tms-refresh-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.tms-spin {
  animation: tms-spin 0.9s linear infinite;
  display: inline-block;
}

@keyframes tms-spin {
  to { transform: rotate(360deg); }
}

/* ── Action buttons ────────────────────────────── */
.tms-actions {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: var(--space-2, 8px);
}

.tms-action-btn {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 12px;
  border: 1px solid var(--bg-tertiary, rgba(15, 15, 15, 0.08));
  background: var(--bg-secondary, #FBFBFB);
  color: var(--text-primary, #333);
  border-radius: var(--radius-md, 10px);
  font-size: var(--font-size-sm, 13px);
  cursor: pointer;
  transition: background 0.15s, transform 0.1s;
  text-align: left;
}

.tms-action-btn:hover:not(:disabled) {
  background: var(--bg-hover, rgba(15, 15, 15, 0.06));
}

.tms-action-btn:active:not(:disabled) {
  transform: scale(0.97);
}

.tms-action-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.tms-action-btn i {
  font-size: 16px;
  color: var(--text-secondary, #666);
  flex-shrink: 0;
}

/* ── Sublist (playlists) ───────────────────────── */
.tms-sublist {
  display: flex;
  flex-direction: column;
  gap: 2px;
  margin-top: var(--space-1, 4px);
}

.tms-sublist-empty {
  padding: var(--space-2, 8px);
  text-align: center;
  color: var(--text-tertiary, #999);
  font-size: var(--font-size-xs, 12px);
}

.tms-sublist-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  border: none;
  background: transparent;
  color: var(--text-primary, #333);
  border-radius: var(--radius-sm, 6px);
  font-size: var(--font-size-sm, 13px);
  cursor: pointer;
  text-align: left;
  width: 100%;
}

.tms-sublist-item:hover {
  background: var(--bg-hover, rgba(15, 15, 15, 0.06));
}

.tms-sublist-item i {
  color: var(--text-tertiary, #999);
  font-size: 14px;
  flex-shrink: 0;
}

.tms-sublist-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tms-sublist-count {
  font-size: var(--font-size-xs, 11px);
  color: var(--text-tertiary, #999);
  flex-shrink: 0;
}

.tms-sublist-create {
  color: var(--text-secondary, #666);
  font-style: italic;
  border-top: 1px dashed var(--bg-tertiary, rgba(15, 15, 15, 0.08));
  margin-top: 4px;
  padding-top: 10px;
}

/* ── Match list (online search) ────────────────── */
.tms-loading,
.tms-empty-inline {
  padding: var(--space-2, 8px) 0;
  color: var(--text-tertiary, #999);
  font-size: var(--font-size-sm, 13px);
  display: flex;
  align-items: center;
  gap: 6px;
}

.tms-match-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
  max-height: 280px;
  overflow-y: auto;
}

.tms-match-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  border: none;
  background: transparent;
  color: var(--text-primary, #333);
  border-radius: var(--radius-sm, 6px);
  cursor: pointer;
  text-align: left;
  width: 100%;
}

.tms-match-item:hover {
  background: var(--bg-hover, rgba(15, 15, 15, 0.06));
}

.tms-match-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.tms-match-title {
  font-size: var(--font-size-sm, 13px);
  color: var(--text-primary, #333);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tms-match-sub {
  font-size: var(--font-size-xs, 11px);
  color: var(--text-tertiary, #999);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tms-match-play {
  font-size: 18px;
  color: var(--text-secondary, #666);
  flex-shrink: 0;
}

/* ── Source list ───────────────────────────────── */
.tms-source-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.tms-source-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 10px 12px;
  border: 1px solid transparent;
  background: var(--bg-secondary, #FBFBFB);
  color: var(--text-primary, #333);
  border-radius: var(--radius-md, 10px);
  cursor: pointer;
  text-align: left;
  width: 100%;
  transition: background 0.15s, border-color 0.15s;
}

.tms-source-item:hover:not(:disabled) {
  background: var(--bg-hover, rgba(15, 15, 15, 0.06));
}

.tms-source-item:disabled {
  cursor: not-allowed;
}

.tms-source-active {
  border-color: var(--primary-color);
  background: var(--bg-active);
}

.tms-source-main {
  display: flex;
  align-items: center;
  gap: 10px;
  flex: 1;
  min-width: 0;
}

.tms-source-main > i {
  font-size: 18px;
  color: var(--text-secondary, #666);
  flex-shrink: 0;
}

.tms-source-text {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.tms-source-name {
  font-size: var(--font-size-sm, 13px);
  font-weight: 500;
  color: var(--text-primary, #333);
}

.tms-source-id {
  font-size: var(--font-size-xs, 11px);
  color: var(--text-tertiary, #999);
  font-family: ui-monospace, "SF Mono", monospace;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tms-source-check {
  color: var(--primary-color);
  font-size: 16px;
  flex-shrink: 0;
}

/* ── Quality info ──────────────────────────────── */
.tms-quality {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: var(--space-2, 8px);
}

.tms-quality-item {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 8px 10px;
  background: var(--bg-secondary, #FBFBFB);
  border-radius: var(--radius-sm, 8px);
}

.tms-quality-label {
  font-size: var(--font-size-xs, 11px);
  color: var(--text-tertiary, #999);
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.tms-quality-value {
  font-size: var(--font-size-sm, 13px);
  color: var(--text-primary, #333);
  font-weight: 500;
}

/* ── Toast ─────────────────────────────────────── */
.tms-toast {
  position: fixed;
  bottom: calc(80px + env(safe-area-inset-bottom, 0px));
  left: 50%;
  transform: translateX(-50%);
  padding: 8px 16px;
  border-radius: var(--radius-full, 999px);
  font-size: var(--font-size-sm, 13px);
  color: #fff;
  background: rgba(0, 0, 0, 0.78);
  z-index: 500;
  pointer-events: none;
  white-space: nowrap;
  max-width: 90vw;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tms-toast--success { background: rgba(40, 167, 69, 0.92); }
.tms-toast--error { background: rgba(220, 53, 69, 0.92); }
.tms-toast--info { background: rgba(0, 122, 255, 0.92); }

.tms-toast-enter-active,
.tms-toast-leave-active {
  transition: opacity 0.2s, transform 0.2s;
}

.tms-toast-enter-from,
.tms-toast-leave-to {
  opacity: 0;
  transform: translateX(-50%) translateY(8px);
}

/* ── Dark mode ─────────────────────────────────── */
@media (prefers-color-scheme: dark) {
  .tms-action-btn,
  .tms-quality-item,
  .tms-source-item {
    background: rgba(255, 255, 255, 0.04);
    color: var(--text-primary, #f6f6f6);
  }
  .tms-action-btn:hover:not(:disabled),
  .tms-sublist-item:hover,
  .tms-match-item:hover,
  .tms-source-item:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.08);
  }
}
</style>
