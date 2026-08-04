<script setup>
/**
 * AppBottomBar - 桌面端底栏播放控制条（全宽）
 *
 * 结构（三栏居中布局，视觉更专业、信息更聚焦）：
 *   ┌ 进度行：当前时间 ── 可拖拽进度条（hover 预览 + tooltip）── 总时长
 *   ├ 左区：封面 + 标题/歌手
 *   ├ 中区（绝对居中）：播放模式 / 上一首 / 播放暂停 / 下一首
 *   └ 右区：收藏 / 播放队列 / 歌词 + 音量（hover 展开百分比）
 *
 * 封面与元信息使用与 PlayerControlBar 相同的 Teleport 槽位，
 * 保证 PlayerView 打开时的"共享 DOM" FLIP 动画仍然生效。
 */
import { computed, ref, useTemplateRef } from 'vue';
import { HugeiconsIcon } from '@hugeicons/vue';
import {
  PlayIcon,
  PauseIcon,
  FastForwardIcon,
  ArrowTurnBackwardIcon,
  ShuffleIcon,
  RepeatIcon,
  RepeatOneIcon,
  RepeatOffIcon,
  VolumeUpIcon,
  VolumeMute01Icon,
  MusicNote01Icon,
  HeartIcon,
  Queue01Icon,
  CaptionsIcon,
} from '@hugeicons/core-free-icons';
import PlayerStore from '@/stores/player/index.js';
import { useCoverImage } from '@/composables/useCoverImage.js';
import { useFavorites } from '@/composables/useFavorites.js';
import { formatDuration } from '@/utils/format.js';

const currentTrack = computed(() => PlayerStore.state.currentTrack);
const { coverUrl: albumCoverUrl } = useCoverImage(currentTrack, 'small');
const isPlaying = computed(() => PlayerStore.state.isPlaying);
const playMode = computed(() => PlayerStore.state.playMode);
const volume = computed(() => PlayerStore.state.volume);
const muted = computed(() => PlayerStore.state.muted);
const currentTime = computed(() => PlayerStore.state.currentTime);
const duration = computed(() => PlayerStore.state.duration);

const trackTitle = computed(() => currentTrack.value?.title ?? '未在播放');
const trackArtist = computed(() => currentTrack.value?.getDisplayArtist?.() ?? currentTrack.value?.artist ?? '');
const trackAlbum = computed(() => currentTrack.value?.albumTitle ?? '');
const trackMeta = computed(() => {
  const parts = [trackArtist.value, trackAlbum.value].filter(Boolean);
  return parts.join(' · ') || '选择一首歌曲开始播放';
});

const progressPct = computed(() => {
  if (!duration.value) return 0;
  return Math.min(100, (currentTime.value / duration.value) * 100);
});

const coverPortalTarget = computed(() => PlayerStore.coverPortalTarget.value);
const metaPortalTarget = computed(() => PlayerStore.metaPortalTarget.value);

const openPlayerView = () => PlayerStore.openPlayerView();
const togglePlay = () => PlayerStore.togglePlay();

// ── 播放模式（四种状态图标区分）────────────────────────
const modeIcon = computed(() => {
  switch (playMode.value) {
    case 'random': return ShuffleIcon;
    case 'loop': return RepeatIcon;
    case 'loop_one': return RepeatOneIcon;
    default: return RepeatOffIcon; // sequence 顺序播放
  }
});
const modeTitle = computed(() => {
  switch (playMode.value) {
    case 'random': return '随机播放';
    case 'loop': return '列表循环';
    case 'loop_one': return '单曲循环';
    default: return '顺序播放';
  }
});

// ── 收藏（响应式，带心形弹跳反馈）─────────────────────
const { favoriteIds, toggleFavorite } = useFavorites();
const favPop = ref(false);
let favPopTimer = null;
const isFavorite = computed(() =>
  currentTrack.value?.id ? favoriteIds.value.includes(currentTrack.value.id) : false
);
async function onToggleFavorite() {
  if (!currentTrack.value?.id) return;
  await toggleFavorite(currentTrack.value.id);
  favPop.value = true;
  clearTimeout(favPopTimer);
  favPopTimer = setTimeout(() => (favPop.value = false), 320);
}

// ── 播放队列 / 歌词快捷入口 ────────────────────────────
const isQueueOpen = computed(() => PlayerStore.isPlayQueueOpen.value);
const isLyricsMode = computed(
  () => PlayerStore.isPlayerViewOpen.value && PlayerStore.playerViewMode.value === 'lyrics'
);
function openQueue() {
  if (!PlayerStore.isPlayerViewOpen.value) PlayerStore.openPlayerView();
  PlayerStore.openPlayQueue();
}
function openLyrics() {
  if (!PlayerStore.isPlayerViewOpen.value) PlayerStore.openPlayerView();
  PlayerStore.setPlayerViewMode('lyrics');
}

// ── 进度条（支持拖拽 + hover 预览 tooltip）──────────────
const seekTrack = useTemplateRef('seekTrack');
const dragging = ref(false);
const hoverPct = ref(-1);   // -1 表示未 hover
const hoverTime = ref(0);

function pctFromPointer(e) {
  const track = seekTrack.value;
  if (!track) return 0;
  const rect = track.getBoundingClientRect();
  const ratio = (e.clientX - rect.left) / rect.width;
  return Math.max(0, Math.min(1, ratio));
}

function seekToPointer(e) {
  if (!duration.value) return;
  PlayerStore.seek(pctFromPointer(e) * duration.value);
}

function onSeekPointerDown(e) {
  if (!duration.value || !seekTrack.value) return;
  e.preventDefault();
  const track = seekTrack.value;
  track.setPointerCapture(e.pointerId);
  dragging.value = true;
  seekToPointer(e);

  const onMove = (ev) => seekToPointer(ev);
  const onUp = (ev) => {
    dragging.value = false;
    try {
      track.releasePointerCapture(ev.pointerId);
    } catch {
      /* 忽略释放失败 */
    }
    track.removeEventListener('pointermove', onMove);
    track.removeEventListener('pointerup', onUp);
    track.removeEventListener('pointercancel', onUp);
  };
  track.addEventListener('pointermove', onMove);
  track.addEventListener('pointerup', onUp);
  track.addEventListener('pointercancel', onUp);
}

function onSeekMove(e) {
  if (dragging.value || !duration.value) return;
  const ratio = pctFromPointer(e);
  hoverPct.value = ratio * 100;
  hoverTime.value = ratio * duration.value;
}
function onSeekLeave() {
  hoverPct.value = -1;
}

// ── 音量 ─────────────────────────────────────────────
const onVolumeChange = (e) => PlayerStore.setVolume(parseFloat(e.target.value));
const volumePct = computed(() => Math.round((muted.value ? 0 : volume.value) * 100));
</script>

<template>
  <div class="app-bottom-bar">
    <!-- 进度行：时间 ── 可拖拽进度条 ── 时间 -->
    <div class="seek-row">
      <span class="time-label">{{ formatDuration(currentTime) }}</span>
      <div
        ref="seekTrack"
        class="seek-track"
        :class="{ dragging, disabled: !duration }"
        @pointerdown="onSeekPointerDown"
        @pointermove="onSeekMove"
        @pointerleave="onSeekLeave"
      >
        <div v-if="hoverPct >= 0" class="seek-hover" :style="{ width: hoverPct + '%' }"></div>
        <div class="seek-fill" :style="{ width: progressPct + '%' }"></div>
        <div class="seek-thumb" :style="{ left: progressPct + '%' }"></div>
        <div
          v-if="hoverPct >= 0"
          class="seek-tooltip"
          :style="{ '--hover-pct': hoverPct + '%' }"
        >{{ formatDuration(hoverTime) }}</div>
      </div>
      <span class="time-label">{{ formatDuration(duration) }}</span>
    </div>

    <!-- 主体三栏 -->
    <div class="bar-inner">
      <!-- 左区：封面 + 元信息（共享 DOM 槽位，勿动 Teleport 结构） -->
      <div class="bar-left">
        <div class="cover-slot">
          <Teleport :to="coverPortalTarget" :disabled="!coverPortalTarget">
            <button
              class="cover-thumb"
              :class="{ placeholder: !albumCoverUrl }"
              data-shared="cover"
              @click="openPlayerView"
              :title="trackTitle"
            >
              <img v-if="albumCoverUrl" :src="albumCoverUrl" alt="专辑封面" />
              <HugeiconsIcon v-else :icon="MusicNote01Icon" :size="18" class="cover-ph-icon" />
            </button>
          </Teleport>
        </div>

        <div class="meta-slot">
          <Teleport :to="metaPortalTarget" :disabled="!metaPortalTarget">
            <div class="track-meta" data-shared="meta">
              <span class="track-title" :title="trackTitle">{{ trackTitle }}</span>
              <span class="track-artist" :title="trackMeta">{{ trackMeta }}</span>
            </div>
          </Teleport>
        </div>
      </div>

      <!-- 中区：播放控制（绝对居中） -->
      <div class="controls">
        <button
          class="ctrl-btn mode"
          :class="{ on: playMode !== 'sequence' }"
          :title="modeTitle"
          @click="PlayerStore.togglePlayMode()"
        >
          <HugeiconsIcon :icon="modeIcon" :size="17" />
        </button>
        <button class="ctrl-btn" title="上一首" @click="PlayerStore.playPrevious()">
          <HugeiconsIcon :icon="ArrowTurnBackwardIcon" :size="19" />
        </button>
        <button class="ctrl-btn play" :title="isPlaying ? '暂停' : '播放'" @click="togglePlay">
          <HugeiconsIcon :icon="isPlaying ? PauseIcon : PlayIcon" :size="24" class="play-ic" />
        </button>
        <button class="ctrl-btn" title="下一首" @click="PlayerStore.playNext()">
          <HugeiconsIcon :icon="FastForwardIcon" :size="19" />
        </button>
      </div>

      <!-- 右区：收藏 / 队列 / 歌词 / 音量 -->
      <div class="bar-right">
        <button
          class="ctrl-btn fav-btn"
          :class="{ active: isFavorite, pop: favPop, disabled: !currentTrack }"
          :disabled="!currentTrack"
          :title="!currentTrack ? '暂无歌曲' : isFavorite ? '取消收藏' : '收藏'"
          @click="onToggleFavorite"
        >
          <HugeiconsIcon :icon="HeartIcon" :size="18" />
        </button>
        <button class="ctrl-btn queue-btn" :class="{ active: isQueueOpen }" title="播放队列" @click="openQueue">
          <HugeiconsIcon :icon="Queue01Icon" :size="18" />
        </button>
        <button class="ctrl-btn lyrics-btn" :class="{ active: isLyricsMode }" title="歌词" @click="openLyrics">
          <HugeiconsIcon :icon="CaptionsIcon" :size="18" />
        </button>

        <div class="volume-group">
          <button class="ctrl-btn" title="静音" @click="PlayerStore.toggleMute()">
            <HugeiconsIcon :icon="muted || volume === 0 ? VolumeMute01Icon : VolumeUpIcon" :size="17" />
          </button>
          <div class="volume-wrap" :title="`音量 ${volumePct}%`">
            <input
              class="volume-slider"
              type="range"
              min="0"
              max="1"
              step="0.01"
              :value="muted ? 0 : volume"
              @input="onVolumeChange"
            />
            <span class="volume-pct">{{ volumePct }}</span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.app-bottom-bar {
  height: var(--bottom-bar-height);
  background: var(--bg-secondary);
  border-top: 1px solid var(--border-light);
  display: flex;
  flex-direction: column;
  position: relative;
  flex-shrink: 0;
  z-index: 90;
}

/* ── 进度行 ─────────────────────────────────────── */
.seek-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 16px 2px;
  flex-shrink: 0;
  user-select: none;
}

.time-label {
  font-size: var(--font-size-xs);
  color: var(--text-tertiary);
  font-variant-numeric: tabular-nums;
  flex-shrink: 0;
  min-width: 32px;
  text-align: center;
}

.seek-track {
  position: relative;
  flex: 1;
  height: 5px;
  border-radius: 999px;
  background: var(--bg-tertiary);
  cursor: pointer;
  touch-action: none;
  transition: height var(--transition-fast);
}
.seek-track:hover {
  height: 7px;
}
.seek-track.disabled {
  cursor: default;
  opacity: 0.55;
}
.seek-track.disabled:hover {
  height: 5px;
}

.seek-fill {
  position: absolute;
  left: 0;
  top: 0;
  bottom: 0;
  background: var(--primary-color);
  border-radius: inherit;
  pointer-events: none;
}

.seek-hover {
  position: absolute;
  left: 0;
  top: 0;
  bottom: 0;
  background: var(--text-tertiary);
  opacity: 0.35;
  width: 0;
  border-radius: inherit;
  pointer-events: none;
}

.seek-thumb {
  position: absolute;
  top: 50%;
  width: 11px;
  height: 11px;
  border-radius: 50%;
  background: var(--primary-color);
  box-shadow: 0 0 0 2px var(--bg-primary), 0 1px 5px rgba(0, 0, 0, 0.35);
  transform: translate(-50%, -50%) scale(0.4);
  opacity: 0;
  transition: opacity var(--transition-fast), transform var(--transition-fast);
  pointer-events: none;
}
.seek-track:hover .seek-thumb,
.seek-track.dragging .seek-thumb {
  opacity: 1;
  transform: translate(-50%, -50%) scale(1);
}

.seek-tooltip {
  position: absolute;
  bottom: calc(100% + 8px);
  left: calc(clamp(14%, var(--hover-pct), 86%));
  transform: translateX(-50%);
  background: rgba(0, 0, 0, 0.82);
  color: #fff;
  font-size: var(--font-size-xs);
  padding: 3px 7px;
  border-radius: 6px;
  pointer-events: none;
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.3);
}

/* ── 主体三栏 ────────────────────────────────────── */
.bar-inner {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 0 16px 8px;
  position: relative;
}

.bar-left {
  flex: 1 1 0;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 12px;
}

.controls {
  position: absolute;
  left: 50%;
  transform: translateX(-50%);
  display: flex;
  align-items: center;
  gap: 2px;
  flex-shrink: 0;
}

.bar-right {
  flex: 1 1 0;
  min-width: 0;
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 2px;
}

/* 封面 */
.cover-slot {
  width: 44px;
  height: 44px;
  flex-shrink: 0;
}

.cover-thumb {
  width: 44px;
  height: 44px;
  border: none;
  border-radius: 6px;
  overflow: hidden;
  padding: 0;
  cursor: pointer;
  background: var(--bg-tertiary);
  display: flex;
  align-items: center;
  justify-content: center;
  box-shadow: var(--shadow-sm);
  transition: transform var(--transition-fast);
}

.cover-thumb:hover {
  transform: scale(1.04);
}

.cover-thumb img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
}

.cover-ph-icon {
  color: var(--text-tertiary);
}

/* 元信息 */
.meta-slot {
  min-width: 0;
  flex: 1;
  max-width: 340px;
}

.track-meta {
  display: flex;
  flex-direction: column;
  min-width: 0;
  line-height: 1.35;
}

.track-title {
  font-size: var(--font-size-md);
  font-weight: 600;
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.track-artist {
  font-size: var(--font-size-sm);
  color: var(--text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* 控制按钮 */
.ctrl-btn {
  width: 34px;
  height: 34px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: var(--radius-md);
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  transition: color var(--transition-fast), background var(--transition-fast), transform var(--transition-fast);
}

.ctrl-btn:hover {
  color: var(--text-primary);
  background: var(--bg-hover);
}

.ctrl-btn:active {
  transform: scale(0.94);
}

.ctrl-btn.play {
  width: 42px;
  height: 42px;
  background: var(--primary-color);
  color: var(--primary-contrast);
  border-radius: var(--radius-full);
}

.ctrl-btn.play:hover {
  background: var(--primary-hover);
  color: var(--primary-contrast);
}

.play-ic {
  margin-left: 1px;
}

/* 模式按钮：非默认（顺序）时高亮 */
.ctrl-btn.mode.on {
  color: var(--primary-color);
}

/* 收藏按钮 */
.fav-btn {
  color: var(--text-secondary);
}
.fav-btn.active {
  color: #e5484d;
}
.fav-btn.active svg path {
  fill: currentColor; /* outline 爱心 → 实心 */
}
.fav-btn.disabled {
  opacity: 0.35;
  cursor: default;
  pointer-events: none;
}
.fav-btn.pop {
  animation: fav-pop 0.32s ease;
}
@keyframes fav-pop {
  0% { transform: scale(1); }
  40% { transform: scale(1.28); }
  100% { transform: scale(1); }
}

/* 队列 / 歌词：激活时高亮 */
.queue-btn.active,
.lyrics-btn.active {
  color: var(--primary-color);
}

/* 音量组 */
.volume-group {
  display: flex;
  align-items: center;
  gap: 2px;
  margin-left: 4px;
}

.volume-wrap {
  display: flex;
  align-items: center;
  gap: 4px;
}

.volume-slider {
  width: 64px;
  height: 4px;
  accent-color: var(--primary-color);
  cursor: pointer;
  transition: width var(--transition-fast);
}

.volume-group:hover .volume-slider {
  width: 96px;
}

.volume-pct {
  font-size: var(--font-size-xs);
  color: var(--text-tertiary);
  font-variant-numeric: tabular-nums;
  min-width: 22px;
  text-align: left;
  opacity: 0;
  transition: opacity var(--transition-fast);
}

.volume-group:hover .volume-pct {
  opacity: 1;
}

/* ── 窄屏适配：逐级隐藏次要按钮，避免挤压 ───────── */
@media (max-width: 1100px) {
  .meta-slot {
    max-width: 240px;
  }
  .lyrics-btn {
    display: none;
  }
}

@media (max-width: 900px) {
  .volume-slider {
    width: 48px;
  }
  .volume-group:hover .volume-slider {
    width: 72px;
  }
  .queue-btn {
    display: none;
  }
}

@media (max-width: 767px) {
  .app-bottom-bar {
    display: none;
  }
}
</style>
