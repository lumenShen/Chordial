<script setup>
import { computed, ref, onMounted, onUnmounted, watch, nextTick, useTemplateRef } from 'vue';
import { useRouter } from 'vue-router';
import { useCoverImages } from '@/composables/useCoverImage';
import { useVirtualList } from '@/composables/useVirtualList';
import { useAnime } from '@/composables/useAnime.js';
import CoverImage from './CoverImage.vue';
import PlayerStore from '@/stores/player.js';
import { usePerf } from '@/utils/performanceMonitor.js';
import * as playlistApi from '@/api/musicSource/playlists.js';
import * as neteaseApi from '@/api/musicSource/netease.js';
import * as sourceApi from '@/api/musicSource/sources.js';
import * as prefApi from '@/api/musicSource/lyricPreference.js';

const { start, end, log } = usePerf('TrackList');

const props = defineProps({
  tracks: {
    type: Array,
    default: () => []
  },
  showAlbum: {
    type: Boolean,
    default: true
  },
  showArtist: {
    type: Boolean,
    default: true
  },
  showDuration: {
    type: Boolean,
    default: true
  },
  // 是否启用虚拟列表
  virtualScroll: {
    type: Boolean,
    default: true
  },
  // 每项高度（用于虚拟列表）
  itemHeight: {
    type: Number,
    default: 64
  },
  // 是否启用右键菜单（默认开启）
  contextMenu: {
    type: Boolean,
    default: true
  }
});

const router = useRouter();
const containerRef = ref(null);

const emit = defineEmits(['play', 'select']);

// anime.js 动画作用域（限定到组件根节点）
const rootRef = useTemplateRef('root');
const { run } = useAnime(() => rootRef.value);

// 使用 composable 批量加载封面
const tracksRef = computed(() => props.tracks);
const { coverUrls } = useCoverImages(tracksRef, 'small');

// 虚拟列表
const {
  visibleItems,
  totalHeight,
  onScroll,
  poolSize
} = useVirtualList(tracksRef, {
  itemHeight: props.itemHeight,
  bufferSize: 5,
  containerRef
});

const handleTrackClick = (track) => {
  emit('select', track);
  router.push(`/track/${track.id}`);
};

const handlePlayClick = (track, event) => {
  event.stopPropagation();
  log('playTrack', { trackId: track.id, source: 'button' });
  emit('play', track);
  // 使用 PlayerStore 播放
  PlayerStore.play(track, props.tracks);
};

const handleTrackDoubleClick = (track) => {
  // 双击播放
  log('playTrack', { trackId: track.id, source: 'dblclick' });
  PlayerStore.play(track, props.tracks);
};

const formatDuration = (seconds) => {
  if (!seconds) return '0:00';
  const mins = Math.floor(seconds / 60);
  const secs = seconds % 60;
  return `${mins}:${secs.toString().padStart(2, '0')}`;
};

const getCoverUrl = (track) => {
  // 优先使用网络来源直接提供的 coverUrl（避免 chordial:// 协议二次拉取）
  if (track.coverUrl) return track.coverUrl;
  // 其次使用从 ResourceManager 加载的封面
  return coverUrls.value.get(track.id) || track.album?.coverUrl || '';
};

// ── 右键菜单 ─────────────────────────────────────────
// 状态：菜单显示位置、目标曲目、歌单列表、子菜单开关
const ctxMenu = ref({
  visible: false,
  x: 0,
  y: 0,
  track: null,
  showPlaylistSub: false,
});

// 主菜单靠近右侧时，二级菜单向左翻转，避免溢出视窗
// 仅在桌面端生效（移动端二级菜单向下展开，由 CSS media query 覆盖）
const submenuFlip = computed(() => {
  if (typeof window === 'undefined') return false;
  const MENU_WIDTH = 220;
  const SUBMENU_WIDTH = 180;
  const MARGIN = 16;
  const menuRightEdge = ctxMenu.value.x + MENU_WIDTH;
  const availableRight = window.innerWidth - menuRightEdge - MARGIN;
  return availableRight < SUBMENU_WIDTH;
});

// 懒加载的歌单列表（首次打开"添加到歌单"时拉取）
const playlists = ref([]);
let playlistsLoaded = false;

// "从云端下载"对话框
// - selectedSource: 当前选中的云端来源名称（registrar 中注册的 web source）
// - webSources: 可用云端来源列表（listWebSources 返回）
// - selectedLevel: 选中的音质等级（standard/higher/exhigh/lossless/hires）
const downloadDialog = ref({
  visible: false,
  track: null,
  loading: false,
  matches: [],
  downloading: false,
  result: null,
  resultQualityLabel: '',
  error: '',
  selectedSource: 'netease',
  webSources: [],
  selectedLevel: 'exhigh',
  levels: [
    { value: 'standard', label: '标准 128kbps MP3' },
    { value: 'higher', label: '较高 192kbps MP3' },
    { value: 'exhigh', label: '极高 320kbps MP3' },
    { value: 'lossless', label: '无损 FLAC 16/44kHz' },
    { value: 'hires', label: 'Hi-Res FLAC 24/96kHz' },
  ],
});

// 懒加载的云端来源列表（首次打开"从云端下载"时拉取）
let webSourcesLoaded = false;

// "查看歌词来源"对话框
const lyricDialog = ref({
  visible: false,
  track: null,
  loading: false,
  data: null,
  error: '',
});

// Toast
const toast = ref(null);
let toastTimer = null;
function showToast(msg, type = 'info', duration = 2500) {
  toast.value = { msg, type };
  if (toastTimer) clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { toast.value = null; }, duration);
}

function onContextMenu(track, event) {
  if (!props.contextMenu) return;
  event.preventDefault();
  event.stopPropagation();

  // 估算菜单尺寸，避免超出视窗（替代原先 overflow-y:auto 的作用）
  const MENU_WIDTH = 220;
  const MENU_HEIGHT = 240;
  const MARGIN = 8;

  let x = event.clientX;
  let y = event.clientY;

  // 右边界
  if (x + MENU_WIDTH > window.innerWidth - MARGIN) {
    x = window.innerWidth - MENU_WIDTH - MARGIN;
  }
  // 下边界
  if (y + MENU_HEIGHT > window.innerHeight - MARGIN) {
    y = window.innerHeight - MENU_HEIGHT - MARGIN;
  }
  if (x < MARGIN) x = MARGIN;
  if (y < MARGIN) y = MARGIN;

  ctxMenu.value = {
    visible: true,
    x,
    y,
    track,
    showPlaylistSub: false,
  };
  // 异步预加载歌单列表（不阻塞菜单显示）
  if (!playlistsLoaded) {
    loadPlaylists();
  }
}

function closeCtxMenu() {
  ctxMenu.value.visible = false;
  ctxMenu.value.showPlaylistSub = false;
}

async function loadPlaylists() {
  try {
    const list = await playlistApi.list();
    playlists.value = list || [];
    playlistsLoaded = true;
  } catch (e) {
    console.warn('加载歌单列表失败:', e);
  }
}

// 全局点击 / Esc 关闭
function onGlobalClick(e) {
  if (!ctxMenu.value.visible) return;
  // 点击在菜单内不关闭
  const menu = document.querySelector('.track-ctx-menu');
  if (menu && menu.contains(e.target)) return;
  closeCtxMenu();
}

function onGlobalKeydown(e) {
  if (e.key === 'Escape') {
    if (ctxMenu.value.visible) closeCtxMenu();
    if (downloadDialog.value.visible) downloadDialog.value.visible = false;
    if (lyricDialog.value.visible) lyricDialog.value.visible = false;
  }
}

onMounted(() => {
  document.addEventListener('click', onGlobalClick, true);
  document.addEventListener('keydown', onGlobalKeydown);
  window.addEventListener('blur', closeCtxMenu);
  window.addEventListener('resize', closeCtxMenu);
});

onUnmounted(() => {
  document.removeEventListener('click', onGlobalClick, true);
  document.removeEventListener('keydown', onGlobalKeydown);
  window.removeEventListener('blur', closeCtxMenu);
  window.removeEventListener('resize', closeCtxMenu);
  if (toastTimer) clearTimeout(toastTimer);
});

// 菜单动作
function ctxPlay() {
  const t = ctxMenu.value.track;
  if (!t) return;
  PlayerStore.play(t, props.tracks);
  closeCtxMenu();
}

function ctxViewDetail() {
  const t = ctxMenu.value.track;
  if (!t) return;
  router.push(`/track/${t.id}`);
  closeCtxMenu();
}

function ctxTogglePlaylistSub() {
  ctxMenu.value.showPlaylistSub = !ctxMenu.value.showPlaylistSub;
}

async function ctxAddToPlaylist(playlistId) {
  const t = ctxMenu.value.track;
  if (!t) return;
  try {
    await playlistApi.addSong(playlistId, t.id);
    showToast(`已添加到歌单`, 'success');
  } catch (e) {
    showToast(`添加失败：${e.message || e}`, 'error');
  } finally {
    closeCtxMenu();
  }
}

async function ctxCreatePlaylistAndAdd() {
  const t = ctxMenu.value.track;
  if (!t) return;
  const name = window.prompt('请输入新歌单名称', '我的收藏');
  if (!name || !name.trim()) return;
  try {
    const pl = await playlistApi.create(name.trim(), null);
    await playlistApi.addSong(pl.id, t.id);
    playlistsLoaded = false; // 重新加载
    await loadPlaylists();
    showToast(`已创建歌单并添加`, 'success');
  } catch (e) {
    showToast(`操作失败：${e.message || e}`, 'error');
  } finally {
    closeCtxMenu();
  }
}

async function ctxDownloadFromCloud() {
  const t = ctxMenu.value.track;
  if (!t) return;
  closeCtxMenu();
  downloadDialog.value = {
    ...downloadDialog.value,
    visible: true,
    track: t,
    loading: true,
    matches: [],
    downloading: false,
    result: null,
    resultQualityLabel: '',
    error: '',
  };
  // 并行：加载云端来源列表（仅首次） + 与本地对比
  try {
    if (!webSourcesLoaded) {
      try {
        const list = await sourceApi.listWebSources();
        downloadDialog.value.webSources = Array.isArray(list) ? list : [];
        if (downloadDialog.value.webSources.length > 0 && !downloadDialog.value.webSources.find((s) => s.name === downloadDialog.value.selectedSource)) {
          downloadDialog.value.selectedSource = downloadDialog.value.webSources[0].name;
        }
      } catch (e) {
        // 拉取来源列表失败不致命，降级为默认 netease
        downloadDialog.value.webSources = [{ name: 'netease', label: '网易云音乐' }];
      }
      webSourcesLoaded = true;
    }
    const title = t.title || '';
    const artist = t.getDisplayArtist ? t.getDisplayArtist() : (t.artist || '');
    const album = t.albumTitle || null;
    const data = await sourceApi.compareWithLocalFromSource(downloadDialog.value.selectedSource, title, artist, album);
    downloadDialog.value.matches = data.matches || [];
    downloadDialog.value.loading = false;
  } catch (e) {
    downloadDialog.value.loading = false;
    downloadDialog.value.error = e.message || String(e);
  }
}

async function ctxDoDownload() {
  const t = downloadDialog.value.track;
  if (!t) return;
  downloadDialog.value.downloading = true;
  downloadDialog.value.error = '';
  try {
    const title = t.title || '';
    const artist = t.getDisplayArtist ? t.getDisplayArtist() : (t.artist || '');
    const sourceName = downloadDialog.value.selectedSource;
    const level = downloadDialog.value.selectedLevel;

    // 先按选中来源搜索以获取 entity_id
    // 当前阶段仅 netease 提供 searchSongs，其他来源需各自实现 search 接口
    let entityId = null;
    if (sourceName === 'netease') {
      const onlineSongs = await neteaseApi.searchSongs(`${title} ${artist}`);
      if (onlineSongs && onlineSongs.length > 0) {
        const sid = onlineSongs[0].sourceIds.find((s) => s.sourceName === 'netease' || !s.isLocal());
        entityId = sid?.entityId;
      }
    } else {
      // 通用来源：通过 compare 接口的 matches 字段获取 entity_id（如果有）
      // 这里假设其他来源未来会提供 searchSongs API；当前先报错让用户感知
      const compareData = await sourceApi.compareWithLocalFromSource(sourceName, title, artist, t.albumTitle || null);
      // 某些来源会在 matches 里附带 source_entity_id
      const match = (compareData.matches || []).find((m) => m.source_entity_id);
      entityId = match?.source_entity_id;
    }

    if (!entityId) {
      downloadDialog.value.error = `${sourceName} 未找到匹配的歌曲`;
      downloadDialog.value.downloading = false;
      return;
    }

    const result = await sourceApi.downloadFromSource(
      sourceName,
      entityId,
      title,
      artist,
      level,
    );
    downloadDialog.value.result = result.path || result;
    downloadDialog.value.resultQualityLabel = result.quality_label || '';
    showToast('下载完成', 'success');
  } catch (e) {
    downloadDialog.value.error = e.message || String(e);
    showToast(`下载失败：${e.message || e}`, 'error');
  } finally {
    downloadDialog.value.downloading = false;
  }
}

function closeDownloadDialog() {
  downloadDialog.value.visible = false;
}

// 当用户切换云端来源时重新对比本地
async function onDownloadSourceChange() {
  const t = downloadDialog.value.track;
  if (!t) return;
  downloadDialog.value.loading = true;
  downloadDialog.value.matches = [];
  downloadDialog.value.error = '';
  try {
    const title = t.title || '';
    const artist = t.getDisplayArtist ? t.getDisplayArtist() : (t.artist || '');
    const album = t.albumTitle || null;
    const data = await sourceApi.compareWithLocalFromSource(downloadDialog.value.selectedSource, title, artist, album);
    downloadDialog.value.matches = data.matches || [];
  } catch (e) {
    downloadDialog.value.error = e.message || String(e);
  } finally {
    downloadDialog.value.loading = false;
  }
}

async function ctxViewLyricSource() {
  const t = ctxMenu.value.track;
  if (!t) return;
  closeCtxMenu();
  lyricDialog.value = {
    visible: true,
    track: t,
    loading: true,
    data: null,
    error: '',
  };
  try {
    const data = await prefApi.getPreferred(t.id);
    lyricDialog.value.data = data;
    lyricDialog.value.loading = false;
  } catch (e) {
    lyricDialog.value.loading = false;
    lyricDialog.value.error = e.message || String(e);
  }
}

function closeLyricDialog() {
  lyricDialog.value.visible = false;
}

// 工具
function sourceLabel(src) {
  if (!src) return '未知';
  const map = { local: '本地', netease: '网易云' };
  return map[src] || src;
}

function formatLabel(fmt) {
  if (!fmt) return '无';
  const map = { yrc: 'YRC 逐字', qrc: 'QRC 逐字', ttml: 'TTML 逐字', lrc: 'LRC 行级', plain: '纯文本' };
  return map[fmt] || fmt;
}

function qualityRankLabel(rank) {
  const map = {
    100: '无损 FLAC',
    90: 'Hi-Res',
    80: '超高 exhigh',
    70: '高 standard',
    60: '中 lower',
    50: '低',
  };
  return map[rank] || `rank=${rank}`;
}

/**
 * 显示匹配项的音质标签。
 *
 * - 若后端返回了 quality_label（统一格式：包含码率+编码格式），优先使用之
 * - 否则若返回了 quality_descriptor（{codec, bitrate_kbps, sample_rate_hz, bit_depth}），按规则拼装
 * - 最后退化为 rank 数字标签
 */
function matchQualityLabel(m) {
  if (m.quality_label) return m.quality_label;
  if (m.quality_descriptor) {
    const d = m.quality_descriptor;
    const codec = codecName(d.codec);
    const parts = [];
    if (d.bitrate_kbps) {
      parts.push(`${d.bitrate_kbps} kbps`);
    }
    parts.push(codec);
    if (d.sample_rate_hz && d.bit_depth) {
      parts.push(`${d.bit_depth}/${Math.round(d.sample_rate_hz / 1000)}kHz`);
    }
    return parts.join(' ');
  }
  return qualityRankLabel(m.quality_rank);
}

function codecName(c) {
  if (!c) return '';
  const map = {
    mp3: 'MP3',
    aac: 'AAC',
    flac: 'FLAC',
    alac: 'ALAC',
    wav: 'WAV',
    ape: 'APE',
    vorbis: 'Ogg',
    opus: 'Opus',
  };
  return map[String(c).toLowerCase()] || String(c).toUpperCase();
}

// --- 动画（anime.js v4）---
// 列表项错峰入场。虚拟列表项用 transform: translateY 定位，动画需排除虚拟项，
// 仅对普通列表项使用 listItemEnter（含 translateY）；动画完成后清除内联 transform 以恢复 CSS hover。
function playEnter() {
  run(({ animate, stagger, presets }) => {
    const root = rootRef.value;
    if (!root) return;
    // 仅普通列表项入场；虚拟列表项依赖 transform 定位，跳过避免覆盖
    const items = root.querySelectorAll('.track-list-body:not(.virtual-scroll) .track-item');
    if (!items.length) return;
    animate(items, {
      ...presets.listItemEnter,
      delay: stagger(50),
      onComplete: () => {
        // 清除内联 transform，恢复 CSS :hover 的背景/上浮效果
        rootRef.value?.querySelectorAll('.track-list-body:not(.virtual-scroll) .track-item').forEach((el) => {
          el.style.transform = '';
        });
      },
    });
  });
}

onMounted(playEnter);
watch(() => props.tracks, () => nextTick(playEnter), { flush: 'post' });
</script>

<template>
  <div ref="root" class="track-list">
    <div class="track-list-header">
      <div class="col-number">#</div>
      <div class="col-title">标题</div>
      <div v-if="showArtist" class="col-artist hide-mobile">歌手</div>
      <div v-if="showAlbum" class="col-album hide-mobile">专辑</div>
      <div v-if="showDuration" class="col-duration">时长</div>
    </div>

    <!-- 虚拟列表容器 -->
    <div
      v-if="virtualScroll && tracks.length > 50"
      ref="containerRef"
      class="track-list-body virtual-scroll"
      @scroll="onScroll"
    >
      <!-- 可视区域 -->
      <div class="viewport" :style="{ height: totalHeight + 'px' }">
        <div
          v-for="{ item: track, index, offsetY } in visibleItems"
          :key="track.id"
          class="track-item virtual-item"
          :style="{ transform: `translateY(${offsetY}px)` }"
          @click="handleTrackClick(track)"
          @dblclick="handleTrackDoubleClick(track)"
          @contextmenu="onContextMenu(track, $event)"
        >
          <div class="col-number">
            <span class="track-number">{{ index + 1 }}</span>
            <button class="play-btn" @click="handlePlayClick(track, $event)">
              <svg viewBox="0 0 24 24" fill="currentColor">
                <path d="M8 5v14l11-7z"/>
              </svg>
            </button>
          </div>

          <div class="col-title">
            <div class="track-cover-wrapper">
              <CoverImage
                :src="getCoverUrl(track)"
                :alt="track.title"
                type="track"
              />
            </div>
            <div class="track-info">
              <div class="track-name">{{ track.title || track.fileName || '未知歌曲' }}</div>
              <div v-if="!showArtist" class="track-artist-mobile">
                {{ track.getDisplayArtist ? track.getDisplayArtist() : (track.artist || '未知歌手') }}
              </div>
            </div>
          </div>

          <div v-if="showArtist" class="col-artist hide-mobile">
            <router-link
              v-if="track.primaryArtist"
              :to="`/artists/${track.primaryArtist.id}`"
              class="artist-link"
              @click.stop
            >
              {{ track.primaryArtist.name }}
            </router-link>
            <span v-else>{{ track.getDisplayArtist ? track.getDisplayArtist() : (track.artist || '未知歌手') }}</span>
          </div>

          <div v-if="showAlbum" class="col-album hide-mobile">
            <router-link
              v-if="track.album"
              :to="`/albums/${track.album.id}`"
              class="album-link"
              @click.stop
            >
              {{ track.album.title }}
            </router-link>
            <span v-else>未知专辑</span>
          </div>

          <div v-if="showDuration" class="col-duration">
            {{ track.getFormattedDuration ? track.getFormattedDuration() : formatDuration(track.duration) }}
          </div>
        </div>
      </div>

      <!-- 性能指示器（开发环境显示） -->
      <div v-if="false" class="perf-indicator">
        DOM: {{ poolSize }} / {{ tracks.length }}
      </div>
    </div>

    <!-- 普通列表（数据量小或禁用虚拟列表时） -->
    <div v-else class="track-list-body">
      <div
        v-for="(track, index) in tracks"
        :key="track.id"
        class="track-item"
        @click="handleTrackClick(track)"
        @dblclick="handleTrackDoubleClick(track)"
        @contextmenu="onContextMenu(track, $event)"
      >
        <div class="col-number">
          <span class="track-number">{{ index + 1 }}</span>
          <button class="play-btn" @click="handlePlayClick(track, $event)">
            <svg viewBox="0 0 24 24" fill="currentColor">
              <path d="M8 5v14l11-7z"/>
            </svg>
          </button>
        </div>

        <div class="col-title">
          <div class="track-cover-wrapper">
            <CoverImage
              :src="getCoverUrl(track)"
              :alt="track.title"
              type="track"
            />
          </div>
          <div class="track-info">
            <div class="track-name">{{ track.title || track.fileName || '未知歌曲' }}</div>
            <div v-if="!showArtist" class="track-artist-mobile">
              {{ track.getDisplayArtist ? track.getDisplayArtist() : (track.artist || '未知歌手') }}
            </div>
          </div>
        </div>

        <div v-if="showArtist" class="col-artist hide-mobile">
          <router-link
            v-if="track.primaryArtist"
            :to="`/artists/${track.primaryArtist.id}`"
            class="artist-link"
            @click.stop
          >
            {{ track.primaryArtist.name }}
          </router-link>
          <span v-else>{{ track.getDisplayArtist ? track.getDisplayArtist() : (track.artist || '未知歌手') }}</span>
        </div>

        <div v-if="showAlbum" class="col-album hide-mobile">
          <router-link
            v-if="track.album"
            :to="`/albums/${track.album.id}`"
            class="album-link"
            @click.stop
          >
            {{ track.album.title }}
          </router-link>
          <span v-else>未知专辑</span>
        </div>

        <div v-if="showDuration" class="col-duration">
          {{ track.getFormattedDuration ? track.getFormattedDuration() : formatDuration(track.duration) }}
        </div>
      </div>
    </div>

    <!-- 右键菜单（Teleport 到 body 避免被父级 overflow 裁剪） -->
    <teleport to="body">
      <div
        v-if="ctxMenu.visible && ctxMenu.track"
        class="track-ctx-menu"
        :style="{ left: ctxMenu.x + 'px', top: ctxMenu.y + 'px' }"
        @click.stop
        @contextmenu.prevent
      >
        <div class="ctx-item" @click="ctxPlay">
          <i class="bi bi-play-fill"></i> 播放
        </div>
        <div class="ctx-item" @click="ctxViewDetail">
          <i class="bi bi-info-circle"></i> 查看详情
        </div>
        <div class="ctx-separator"></div>
        <div class="ctx-item has-sub" @click="ctxTogglePlaylistSub" @mouseenter="ctxMenu.showPlaylistSub = true">
          <i class="bi bi-music-note-list"></i> 添加到歌单
          <i class="bi bi-chevron-right ctx-arrow"></i>
          <!-- 子菜单 -->
          <div
            v-if="ctxMenu.showPlaylistSub"
            class="ctx-submenu"
            :class="{ 'ctx-submenu-left': submenuFlip }"
            @click.stop
          >
            <div v-if="playlists.length === 0" class="ctx-sub-item muted">
              暂无歌单
            </div>
            <div
              v-for="pl in playlists"
              :key="pl.id"
              class="ctx-sub-item"
              @click="ctxAddToPlaylist(pl.id)"
            >
              <i class="bi" :class="pl.source === 'netease' ? 'bi-cloud' : 'bi-hdd'"></i>
              {{ pl.name }}
              <span class="ctx-sub-count">{{ (pl.song_ids || []).length }}</span>
            </div>
            <div class="ctx-separator"></div>
            <div class="ctx-sub-item" @click="ctxCreatePlaylistAndAdd">
              <i class="bi bi-plus-lg"></i> 新建歌单…
            </div>
          </div>
        </div>
        <div class="ctx-item" @click="ctxDownloadFromCloud">
          <i class="bi bi-cloud-download"></i> 从云端下载…
        </div>
        <div class="ctx-item" @click="ctxViewLyricSource">
          <i class="bi bi-card-text"></i> 查看歌词来源…
        </div>
      </div>

      <!-- 下载对话框 -->
      <div v-if="downloadDialog.visible" class="ctx-dialog-mask" @click.self="closeDownloadDialog">
        <div class="ctx-dialog">
          <div class="ctx-dialog-header">
            <h3 class="ctx-dialog-title">
              <i class="bi bi-cloud-download"></i>
              从云端下载
            </h3>
            <button class="ctx-close-btn" @click="closeDownloadDialog">
              <i class="bi bi-x-lg"></i>
            </button>
          </div>
          <div class="ctx-dialog-body">
            <div class="ctx-track-info" v-if="downloadDialog.track">
              <strong>{{ downloadDialog.track.title }}</strong>
              <span> {{ downloadDialog.track.getDisplayArtist ? downloadDialog.track.getDisplayArtist() : (downloadDialog.track.artist || '') }}</span>
            </div>

            <!-- 云端来源选择 -->
            <div class="ctx-section">
              <label class="ctx-field-label">云端来源</label>
              <select
                v-model="downloadDialog.selectedSource"
                class="ctx-select"
                :disabled="downloadDialog.loading || downloadDialog.downloading"
                @change="onDownloadSourceChange"
              >
                <option
                  v-for="s in downloadDialog.webSources"
                  :key="s.name"
                  :value="s.name"
                >
                  {{ s.label || s.name }}
                </option>
              </select>
            </div>

            <div v-if="downloadDialog.loading" class="ctx-loading">
              <div class="ctx-spinner"></div> 正在对比本地音质…
            </div>

            <div v-else-if="downloadDialog.error" class="ctx-error">
              <i class="bi bi-exclamation-triangle"></i> {{ downloadDialog.error }}
            </div>

            <div v-else-if="downloadDialog.result" class="ctx-success">
              <i class="bi bi-check-circle-fill"></i>
              <div>
                <div>下载完成</div>
                <code class="ctx-path">{{ downloadDialog.result }}</code>
                <div v-if="downloadDialog.resultQualityLabel" class="ctx-result-quality">
                  音质：{{ downloadDialog.resultQualityLabel }}
                </div>
              </div>
            </div>

            <template v-else>
              <!-- 本地匹配结果 -->
              <div v-if="downloadDialog.matches.length > 0" class="ctx-section">
                <h4 class="ctx-section-title">本地已有 {{ downloadDialog.matches.length }} 个匹配</h4>
                <div
                  v-for="m in downloadDialog.matches"
                  :key="m.song_id"
                  class="ctx-match-row"
                >
                  <div class="ctx-match-info">
                    <div class="ctx-match-title">{{ m.title }}</div>
                    <div class="ctx-match-meta">
                      {{ (m.artist_names || []).join(', ') }}
                      <span v-if="m.album_title"> · {{ m.album_title }}</span>
                    </div>
                    <div class="ctx-match-quality" :class="{ high: (m.quality_rank || 0) >= 90 }">
                      {{ matchQualityLabel(m) }}
                    </div>
                  </div>
                </div>
                <p class="ctx-hint">
                  <i class="bi bi-info-circle"></i>
                  本地音质已较高时，无需重复下载
                </p>
              </div>
              <div v-else class="ctx-section">
                <p class="ctx-hint">本地未找到匹配，可直接下载云端版本</p>
              </div>

              <div class="ctx-section">
                <label class="ctx-field-label">下载音质</label>
                <select
                  v-model="downloadDialog.selectedLevel"
                  class="ctx-select"
                  :disabled="downloadDialog.downloading"
                >
                  <option
                    v-for="lv in downloadDialog.levels"
                    :key="lv.value"
                    :value="lv.value"
                  >
                    {{ lv.label }}
                  </option>
                </select>
                <p class="ctx-hint">实际音质由云端来源决定，更高等级可能需要 VIP 或受限</p>
              </div>
            </template>
          </div>
          <div class="ctx-dialog-actions" v-if="!downloadDialog.loading && !downloadDialog.result">
            <button class="ctx-btn ctx-btn-secondary" @click="closeDownloadDialog" :disabled="downloadDialog.downloading">
              取消
            </button>
            <button
              class="ctx-btn ctx-btn-primary"
              @click="ctxDoDownload"
              :disabled="downloadDialog.downloading || !!downloadDialog.error"
            >
              <span v-if="downloadDialog.downloading" class="ctx-spinner-small"></span>
              {{ downloadDialog.downloading ? '下载中…' : '下载' }}
            </button>
          </div>
          <div v-else-if="downloadDialog.result" class="ctx-dialog-actions">
            <button class="ctx-btn ctx-btn-primary" @click="closeDownloadDialog">完成</button>
          </div>
        </div>
      </div>

      <!-- 歌词来源对话框 -->
      <div v-if="lyricDialog.visible" class="ctx-dialog-mask" @click.self="closeLyricDialog">
        <div class="ctx-dialog">
          <div class="ctx-dialog-header">
            <h3 class="ctx-dialog-title">
              <i class="bi bi-card-text"></i>
              歌词来源
            </h3>
            <button class="ctx-close-btn" @click="closeLyricDialog">
              <i class="bi bi-x-lg"></i>
            </button>
          </div>
          <div class="ctx-dialog-body">
            <div class="ctx-track-info" v-if="lyricDialog.track">
              <strong>{{ lyricDialog.track.title }}</strong>
              <span> {{ lyricDialog.track.getDisplayArtist ? lyricDialog.track.getDisplayArtist() : (lyricDialog.track.artist || '') }}</span>
            </div>

            <div v-if="lyricDialog.loading" class="ctx-loading">
              <div class="ctx-spinner"></div> 正在查询歌词来源…
            </div>

            <div v-else-if="lyricDialog.error" class="ctx-error">
              <i class="bi bi-exclamation-triangle"></i> {{ lyricDialog.error }}
            </div>

            <div v-else-if="lyricDialog.data" class="ctx-lyric-info">
              <div class="ctx-info-row">
                <span class="ctx-info-label">已选来源</span>
                <span class="ctx-info-value" :class="lyricDialog.data.source">
                  {{ sourceLabel(lyricDialog.data.source) }}
                </span>
              </div>
              <div class="ctx-info-row">
                <span class="ctx-info-label">歌词格式</span>
                <span class="ctx-info-value">
                  {{ formatLabel(lyricDialog.data.format) }}
                  <span v-if="lyricDialog.data.is_word_by_word" class="ctx-tag">逐字</span>
                </span>
              </div>
              <div class="ctx-info-row">
                <span class="ctx-info-label">候选数</span>
                <span class="ctx-info-value">{{ lyricDialog.data.candidates_count }}</span>
              </div>
              <p class="ctx-hint">
                <i class="bi bi-info-circle"></i>
                在「设置 → 网易云音乐来源 → 歌词优先级」可调整来源与格式偏好
              </p>
            </div>

            <div v-else class="ctx-hint">
              <i class="bi bi-emoji-frown"></i> 暂无歌词
            </div>
          </div>
          <div class="ctx-dialog-actions">
            <button class="ctx-btn ctx-btn-primary" @click="closeLyricDialog">关闭</button>
          </div>
        </div>
      </div>

      <!-- Toast -->
      <transition name="ctx-fade">
        <div v-if="toast" class="ctx-toast" :class="toast.type">
          <i class="bi" :class="{
            success: 'bi-check-circle-fill',
            error: 'bi-x-circle-fill',
            info: 'bi-info-circle-fill',
          }[toast.type]"></i>
          <span>{{ toast.msg }}</span>
        </div>
      </transition>
    </teleport>
  </div>
</template>

<style scoped>
.track-list {
  background: var(--bg-secondary);
  border-radius: var(--radius-lg);
  overflow: hidden;
  border: 1px solid var(--border-light);
  box-shadow: var(--shadow-card);
}

.track-list-header {
  display: grid;
  grid-template-columns: 50px 1fr 120px 120px 60px;
  gap: 12px;
  padding: 14px 20px;
  border-bottom: 1px solid var(--border-light);
  font-size: 11px;
  font-weight: 600;
  color: var(--text-tertiary);
  text-transform: uppercase;
  letter-spacing: 0.8px;
}

.track-list-header .col-artist,
.track-list-header .col-album {
  display: none;
}

.track-list-body {
  display: flex;
  flex-direction: column;
}

/* 虚拟列表样式 */
.track-list-body.virtual-scroll {
  position: relative;
  overflow-y: auto;
  overflow-x: hidden;
  max-height: 600px;
  -webkit-overflow-scrolling: touch;
}

.viewport {
  position: relative;
  width: 100%;
}

.track-item {
  display: grid;
  grid-template-columns: 50px 1fr 120px 120px 60px;
  gap: 12px;
  padding: 14px 20px;
  align-items: center;
  cursor: pointer;
  transition: background var(--transition-fast);
}

/* 虚拟列表项使用绝对定位 */
.track-item.virtual-item {
  position: absolute;
  left: 0;
  right: 0;
  height: v-bind(itemHeight + 'px');
  contain: layout style paint;
}

.track-item:hover {
  background: var(--bg-hover);
}

.track-item:hover .track-number {
  display: none;
}

.track-item:hover .play-btn {
  display: flex;
}

.col-number {
  display: flex;
  align-items: center;
  justify-content: center;
  position: relative;
}

.track-number {
  font-size: 14px;
  font-weight: 500;
  color: var(--text-tertiary);
}

.play-btn {
  display: none;
  width: 32px;
  height: 32px;
  border: none;
  border-radius: 50%;
  background: var(--primary-color);
  color: white;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  position: absolute;
  transition: transform var(--transition-fast), box-shadow var(--transition-fast);
  box-shadow: var(--shadow-sm);
}

.play-btn:hover {
  transform: scale(1.1);
  box-shadow: var(--shadow-primary);
}

.play-btn svg {
  width: 16px;
  height: 16px;
}

.col-title {
  display: flex;
  align-items: center;
  gap: 12px;
  min-width: 0;
}

.track-cover-wrapper {
  width: 44px;
  height: 44px;
  flex-shrink: 0;
  border-radius: var(--radius-sm);
  overflow: hidden;
  box-shadow: var(--shadow-sm);
}

.track-info {
  min-width: 0;
}

.track-name {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  letter-spacing: -0.2px;
}

.track-artist-mobile {
  font-size: 12px;
  color: var(--text-secondary);
  margin-top: 3px;
  font-weight: 500;
}

.col-artist,
.col-album {
  font-size: 14px;
  color: var(--text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  font-weight: 500;
}

.artist-link,
.album-link {
  color: inherit;
  transition: color var(--transition-fast);
}

.artist-link:hover,
.album-link:hover {
  color: var(--primary-color);
  text-decoration: none;
}

.col-duration {
  font-size: 14px;
  color: var(--text-tertiary);
  text-align: right;
  font-weight: 500;
  font-variant-numeric: tabular-nums;
}

/* 性能指示器 */
.perf-indicator {
  position: fixed;
  bottom: 80px;
  right: 20px;
  background: rgba(0, 0, 0, 0.7);
  color: #fff;
  padding: 4px 8px;
  border-radius: 4px;
  font-size: 12px;
  font-family: monospace;
  pointer-events: none;
  z-index: 100;
}

@media (min-width: 768px) {
  .track-list-header .col-artist,
  .track-list-header .col-album {
    display: block;
  }
}

@media (max-width: 767px) {
  .track-list-header {
    grid-template-columns: 40px 1fr 60px;
  }

  .track-item {
    grid-template-columns: 40px 1fr 60px;
  }

  .track-cover-wrapper {
    width: 36px;
    height: 36px;
  }
}

@media (prefers-color-scheme: dark) {
  .track-list {
    background: var(--bg-secondary);
    border-color: var(--border-light);
  }

  .track-name {
    color: var(--text-primary);
  }
}
</style>

<!-- 非作用域样式：teleport 到 body 的右键菜单 / 对话框 / Toast -->
<style>
.track-ctx-menu {
  position: fixed;
  z-index: 9999;
  min-width: 200px;
  background: var(--bg-secondary, #fff);
  border: 1px solid var(--border-light, #e8e8e8);
  border-radius: 10px;
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.16);
  padding: 4px;
  font-size: 13px;
  color: var(--text-primary, #333);
  user-select: none;
  /* 防止超出视窗：仅限制宽度，高度由 JS 边界检测处理 */
  max-width: calc(100vw - 16px);
  /* 注意：不要设置 overflow-y:auto，否则会裁剪掉绝对定位的二级菜单 */
}

.ctx-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  border-radius: 6px;
  cursor: pointer;
  position: relative;
  white-space: nowrap;
}

.ctx-item:hover {
  background: var(--bg-hover, rgba(0, 0, 0, 0.04));
}

.ctx-item.has-sub {
  cursor: pointer;
}

.ctx-arrow {
  margin-left: auto;
  font-size: 10px;
  color: var(--text-tertiary, #999);
}

.ctx-submenu {
  position: absolute;
  left: 100%;
  top: -4px;
  min-width: 180px;
  /* 歌单可能很多，给予最大高度并允许滚动 */
  max-height: calc(100vh - 32px);
  overflow-y: auto;
  background: var(--bg-secondary, #fff);
  border: 1px solid var(--border-light, #e8e8e8);
  border-radius: 10px;
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.16);
  padding: 4px;
  margin-left: 4px;
}

/* 主菜单靠近视窗右侧时，二级菜单向左展开，避免溢出 */
.ctx-submenu-left {
  left: auto;
  right: 100%;
  margin-left: 0;
  margin-right: 4px;
}

.ctx-sub-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 12px;
  border-radius: 6px;
  cursor: pointer;
  white-space: nowrap;
  font-size: 13px;
}

.ctx-sub-item:hover {
  background: var(--bg-hover, rgba(0, 0, 0, 0.04));
}

.ctx-sub-item.muted {
  color: var(--text-tertiary, #999);
  cursor: default;
}

.ctx-sub-item.muted:hover {
  background: transparent;
}

.ctx-sub-count {
  margin-left: auto;
  font-size: 11px;
  color: var(--text-tertiary, #999);
  font-variant-numeric: tabular-nums;
}

.ctx-separator {
  height: 1px;
  background: var(--border-light, #e8e8e8);
  margin: 4px 0;
}

/* 对话框 */
.ctx-dialog-mask {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 10000;
  padding: 20px;
}

.ctx-dialog {
  width: 100%;
  max-width: 520px;
  max-height: 90vh;
  background: var(--bg-secondary, #fff);
  border-radius: 16px;
  box-shadow: 0 16px 48px rgba(0, 0, 0, 0.2);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.ctx-dialog-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 16px 20px;
  border-bottom: 1px solid var(--border-light, #e8e8e8);
}

.ctx-dialog-title {
  font-size: 16px;
  font-weight: 700;
  margin: 0;
  color: var(--text-primary, #333);
  display: flex;
  align-items: center;
  gap: 8px;
}

.ctx-close-btn {
  width: 32px;
  height: 32px;
  border: none;
  background: transparent;
  color: var(--text-secondary, #666);
  cursor: pointer;
  border-radius: 50%;
  font-size: 14px;
}

.ctx-close-btn:hover {
  background: var(--bg-hover, rgba(0, 0, 0, 0.04));
  color: var(--text-primary, #333);
}

.ctx-dialog-body {
  padding: 20px;
  overflow-y: auto;
  flex: 1;
}

.ctx-track-info {
  padding: 10px 14px;
  background: var(--bg-hover, rgba(0, 0, 0, 0.02));
  border-radius: 8px;
  margin-bottom: 16px;
  font-size: 14px;
}

.ctx-track-info strong {
  color: var(--text-primary, #333);
}

.ctx-track-info span {
  color: var(--text-secondary, #666);
  margin-left: 4px;
  font-size: 13px;
}

.ctx-loading,
.ctx-error,
.ctx-success {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 12px 14px;
  border-radius: 8px;
  font-size: 13px;
}

.ctx-loading {
  color: var(--text-secondary, #666);
}

.ctx-error {
  background: rgba(220, 53, 69, 0.08);
  color: #dc3545;
}

.ctx-success {
  background: rgba(40, 167, 69, 0.08);
  color: #28a745;
}

.ctx-spinner {
  width: 14px;
  height: 14px;
  border: 2px solid var(--border-light, #e8e8e8);
  border-top-color: var(--primary-color, #0078d7);
  border-radius: 50%;
  animation: ctx-spin 0.8s linear infinite;
  display: inline-block;
  flex-shrink: 0;
}

.ctx-spinner-small {
  display: inline-block;
  width: 12px;
  height: 12px;
  border: 2px solid rgba(255, 255, 255, 0.3);
  border-top-color: #fff;
  border-radius: 50%;
  animation: ctx-spin 0.8s linear infinite;
  margin-right: 6px;
  vertical-align: middle;
}

@keyframes ctx-spin {
  to { transform: rotate(360deg); }
}

.ctx-section {
  margin-bottom: 16px;
}

.ctx-section-title {
  font-size: 13px;
  font-weight: 700;
  color: var(--text-primary, #333);
  margin: 0 0 8px;
}

.ctx-match-row {
  padding: 10px 12px;
  background: var(--bg-hover, rgba(0, 0, 0, 0.02));
  border-radius: 8px;
  margin-bottom: 6px;
}

.ctx-match-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary, #333);
}

.ctx-match-meta {
  font-size: 11px;
  color: var(--text-secondary, #666);
  margin-top: 2px;
}

.ctx-match-quality {
  display: inline-block;
  margin-top: 6px;
  padding: 2px 8px;
  border-radius: 8px;
  font-size: 11px;
  font-weight: 600;
  background: var(--bg-hover, rgba(0, 0, 0, 0.06));
  color: var(--text-secondary, #666);
}

.ctx-match-quality.high {
  background: rgba(40, 167, 69, 0.12);
  color: #28a745;
}

.ctx-hint {
  font-size: 12px;
  color: var(--text-tertiary, #999);
  margin: 8px 0 0;
  line-height: 1.6;
}

.ctx-hint i {
  margin-right: 4px;
}

.ctx-field-label {
  display: block;
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary, #666);
  margin-bottom: 6px;
}

.ctx-select {
  width: 100%;
  padding: 8px 10px;
  font-size: 13px;
  color: var(--text-primary, #333);
  background: var(--bg-input, #fff);
  border: 1px solid var(--border-color, rgba(0, 0, 0, 0.1));
  border-radius: 8px;
  outline: none;
  cursor: pointer;
  transition: border-color 0.2s;
}

.ctx-select:hover:not(:disabled) {
  border-color: var(--accent-color, #007aff);
}

.ctx-select:focus {
  border-color: var(--accent-color, #007aff);
}

.ctx-select:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.ctx-result-quality {
  margin-top: 6px;
  font-size: 12px;
  color: #28a745;
  font-weight: 600;
}

.ctx-path {
  display: block;
  margin-top: 4px;
  padding: 6px 8px;
  font-size: 11px;
  background: var(--bg-hover, rgba(0, 0, 0, 0.04));
  border-radius: 4px;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  color: var(--text-secondary, #666);
  word-break: break-all;
}

.ctx-lyric-info {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.ctx-info-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 14px;
  background: var(--bg-hover, rgba(0, 0, 0, 0.02));
  border-radius: 8px;
  font-size: 13px;
}

.ctx-info-label {
  color: var(--text-secondary, #666);
}

.ctx-info-value {
  font-weight: 600;
  color: var(--text-primary, #333);
  display: flex;
  align-items: center;
  gap: 6px;
}

.ctx-info-value.netease {
  color: #c50000;
}

.ctx-info-value.local {
  color: #28a745;
}

.ctx-tag {
  font-size: 10px;
  padding: 1px 6px;
  border-radius: 8px;
  background: var(--primary-light, rgba(0, 120, 215, 0.1));
  color: var(--primary-color, #0078d7);
  font-weight: 600;
}

.ctx-dialog-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding: 12px 20px;
  border-top: 1px solid var(--border-light, #e8e8e8);
}

.ctx-btn {
  padding: 8px 16px;
  font-size: 13px;
  font-weight: 500;
  border: 1px solid var(--border-color, #e8e8e8);
  border-radius: 8px;
  cursor: pointer;
  transition: all 0.2s ease;
  background: var(--bg-primary, #fff);
  color: var(--text-primary, #333);
  display: inline-flex;
  align-items: center;
}

.ctx-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.ctx-btn-secondary:hover:not(:disabled) {
  border-color: var(--primary-color, #0078d7);
  color: var(--primary-color, #0078d7);
}

.ctx-btn-primary {
  background: var(--primary-color, #0078d7);
  border-color: var(--primary-color, #0078d7);
  color: #fff;
}

.ctx-btn-primary:hover:not(:disabled) {
  background: var(--primary-color-dark, #0066bf);
}

/* Toast */
.ctx-toast {
  position: fixed;
  bottom: 32px;
  left: 50%;
  transform: translateX(-50%);
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px 20px;
  background: var(--bg-secondary, #fff);
  border: 1px solid var(--border-light, #e8e8e8);
  border-radius: 12px;
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.12);
  font-size: 13px;
  z-index: 10001;
  max-width: 90vw;
}

.ctx-toast.success {
  color: #28a745;
  border-color: #28a745;
}

.ctx-toast.error {
  color: #dc3545;
  border-color: #dc3545;
}

.ctx-toast.info {
  color: var(--primary-color, #0078d7);
}

.ctx-fade-enter-active,
.ctx-fade-leave-active {
  transition: opacity 0.2s;
}

.ctx-fade-enter-from,
.ctx-fade-leave-to {
  opacity: 0;
}

@media (max-width: 600px) {
  .track-ctx-menu {
    min-width: 180px;
  }

  .ctx-submenu,
  .ctx-submenu-left {
    left: 0;
    right: auto;
    top: 100%;
    margin-left: 0;
    margin-right: 0;
    margin-top: 4px;
  }

  .ctx-dialog {
    max-width: 100%;
  }
}
</style>
