<script setup>
/**
 * PlayQueueSheet — AMLL bottomControls 播放列表按钮触发的播放队列 Sheet
 *
 * 由 PlayerView.vue 内挂载，监听 PlayerStore.state.ui.isPlayQueueOpen 显示。
 * 触发链路：AMLL horizontalLayout bottomControls 第一个按钮（type="playlist"）
 *        → PlayerView 中的 click 事件委托
 *        → PlayerStore.openPlayQueue() → isPlayQueueOpen=true → 本组件 BottomSheet 显示
 *
 * 设计要点：
 *   - 不复用 TrackList.vue（其单击会跳转到 /track/:id 详情页，不适合队列场景）
 *   - 自实现简洁列表：序号 + 封面 + 标题/艺术家 + 时长
 *   - 当前播放项高亮（左侧强调条 + 背景色 + 序号替换为播放图标）
 *   - 单击直接播放（不跳转）
 *   - Sheet 打开 / 当前曲目变化时自动滚动到当前项
 *   - 使用虚拟列表（useVirtualList）只渲染可见区域 + 缓冲区，保证大队列滚动流畅
 */
import { ref, computed, watch, nextTick, shallowRef, onUnmounted } from 'vue';
import BottomSheet from '@/components/ui/BottomSheet.vue';
import { PlayerStore } from '@/stores/player.js';
import { useCoverImages } from '@/composables/useCoverImage';
import { useVirtualList } from '@/composables/useVirtualList';

// ── Sheet 可见性 ────────────────────────────────────────────
const visible = computed({
  get: () => PlayerStore.state.ui.isPlayQueueOpen,
  set: (v) => (v ? PlayerStore.openPlayQueue() : PlayerStore.closePlayQueue()),
});

// ── 队列数据 ────────────────────────────────────────────────
// shallowRef：playlist 内的 Track 实例已 markRaw，无需深代理
const playlist = shallowRef([]);
const currentTrack = shallowRef(null);
const currentIndex = ref(-1);

watch(
  () => [PlayerStore.state.playlist, PlayerStore.state.currentTrack, PlayerStore.state.currentIndex],
  ([list, track, idx]) => {
    playlist.value = list ?? [];
    currentTrack.value = track ?? null;
    currentIndex.value = typeof idx === 'number' ? idx : -1;
  },
  { immediate: true, deep: false },
);

// ── 封面批量加载（small 尺寸，复用 composable 缓存）─────────
const tracksRef = computed(() => playlist.value);
const { coverUrls } = useCoverImages(tracksRef, 'small');

function getCoverUrl(track) {
  if (!track) return '';
  // 1. track 自带的网络封面 URL（在线歌曲）
  if (track.coverUrl) return track.coverUrl;
  // 2. composable 解析出的 chordial:// 封面 URL
  return coverUrls.value.get(track.id) || '';
}

// ── 时长格式化 ──────────────────────────────────────────────
function formatDuration(sec) {
  if (!sec || isNaN(sec)) return '--:--';
  const m = Math.floor(sec / 60);
  const s = Math.floor(sec % 60);
  return `${m}:${s.toString().padStart(2, '0')}`;
}

// ── 交互 ────────────────────────────────────────────────────
function playTrack(track) {
  if (!track) return;
  // 传入当前 playlist，确保 PlayerStore 内部 playlist 引用不变（避免重置 markRaw 数组）
  PlayerStore.play(track, playlist.value);
}

// ── 虚拟列表 ────────────────────────────────────────────────
// 每项固定高度 56px = padding-top(8) + cover(40) + padding-bottom(8)
// 桌面/移动端一致（移动端仅横向 grid 列宽变化，纵向高度不变）
const ITEM_HEIGHT = 56;
const containerRef = ref(null);

const {
  visibleItems,
  totalHeight,
  onScroll,
  scrollToIndex,
  updateContainerHeight,
} = useVirtualList(tracksRef, {
  itemHeight: ITEM_HEIGHT,
  bufferSize: 5,
  containerRef,
  mode: 'container',
});

// 监听容器尺寸变化（BottomSheet 入场动画、窗口缩放等），
// useVirtualList 内部仅监听 window resize，无法捕获 sheet 动画期间的高度变化。
let resizeObserver = null;
watch(containerRef, (el, oldEl) => {
  if (oldEl && resizeObserver) {
    resizeObserver.disconnect();
    resizeObserver = null;
  }
  if (el) {
    resizeObserver = new ResizeObserver(() => updateContainerHeight());
    resizeObserver.observe(el);
  }
});

onUnmounted(() => {
  if (resizeObserver) {
    resizeObserver.disconnect();
    resizeObserver = null;
  }
});

// ── 自动滚动到当前项 ────────────────────────────────────────
// 虚拟列表下当前项可能不在 DOM 中，无法用 scrollIntoView；
// 改用 scrollToIndex 直接定位到当前索引。
async function scrollCurrentIntoView() {
  await nextTick();
  if (currentIndex.value < 0) return;
  updateContainerHeight();
  scrollToIndex(currentIndex.value);
}

// Sheet 打开时或当前曲目变化时滚动定位
watch(
  () => [visible.value, currentIndex.value],
  ([isOpen]) => {
    if (isOpen) scrollCurrentIntoView();
  },
);

// ── 关闭回调（清理）────────────────────────────────────────
function onClose() {
  // BottomSheet 已通过 v-model 双向同步 state，这里仅作为钩子预留
}
</script>

<template>
  <BottomSheet
    v-model:visible="visible"
    title="播放队列"
    :detents="['large']"
    default-detent="large"
    @close="onClose"
  >
    <div v-if="playlist.length === 0" class="pqs-empty">
      <i class="bi bi-music-note-list" />
      <span>播放队列为空</span>
    </div>

    <div v-else class="pqs-root">
      <!-- 队列信息头 -->
      <header class="pqs-header">
        <span class="pqs-count">
          共 {{ playlist.length }} 首
        </span>
        <span v-if="currentIndex >= 0" class="pqs-current">
          正在播放第 {{ currentIndex + 1 }} 首
        </span>
      </header>

      <!-- 虚拟列表滚动容器 -->
      <div
        ref="containerRef"
        class="pqs-list"
        @scroll="onScroll"
      >
        <!-- 可视区域：用 totalHeight 撑开真实滚动条高度 -->
        <div class="pqs-viewport" :style="{ height: totalHeight + 'px' }">
          <div
            v-for="{ item: track, index, offsetY } in visibleItems"
            :key="track.id"
            class="pqs-item pqs-item--virtual"
            :class="{ 'pqs-item--current': index === currentIndex }"
            :style="{ transform: `translateY(${offsetY}px)` }"
            @click="playTrack(track)"
          >
            <!-- 序号 / 播放图标 -->
            <div class="pqs-col-number">
              <template v-if="index === currentIndex">
                <span class="pqs-playing-icon" :class="{ 'is-playing': PlayerStore.state.isPlaying }">
                  <span class="bar" />
                  <span class="bar" />
                  <span class="bar" />
                </span>
              </template>
              <span v-else class="pqs-index">{{ index + 1 }}</span>
            </div>

            <!-- 封面 -->
            <div class="pqs-cover">
              <img
                v-if="getCoverUrl(track)"
                :src="getCoverUrl(track)"
                :alt="track.title"
                loading="lazy"
              />
              <i v-else class="bi bi-music-note-beamed pqs-cover-placeholder" />
            </div>

            <!-- 标题 + 艺术家 -->
            <div class="pqs-info">
              <div class="pqs-title">{{ track.title || '未知歌曲' }}</div>
              <div class="pqs-artist">
                {{ track.getDisplayArtist ? track.getDisplayArtist() : (track.artist || '未知歌手') }}
              </div>
            </div>

            <!-- 时长 -->
            <div class="pqs-duration">
              {{ track.getFormattedDuration ? track.getFormattedDuration() : formatDuration(track.duration) }}
            </div>
          </div>
        </div>
      </div>
    </div>
  </BottomSheet>
</template>

<style scoped>
/* ── 容器 ──────────────────────────────────────────────────── */
.pqs-root {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}

.pqs-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--space-3, 12px);
  padding: var(--space-8, 32px);
  color: var(--text-tertiary, #999);
  font-size: var(--font-size-md, 14px);
}

.pqs-empty i {
  font-size: 48px;
  opacity: 0.4;
}

/* ── 头部 ──────────────────────────────────────────────────── */
.pqs-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-2, 8px) var(--space-4, 16px) var(--space-3, 12px);
  border-bottom: 1px solid var(--bg-tertiary, rgba(255, 255, 255, 0.08));
  font-size: var(--font-size-xs, 12px);
  color: var(--text-secondary, #999);
}

.pqs-count {
  font-weight: 500;
}

.pqs-current {
  color: var(--accent-primary, #ff375f);
}

/* ── 列表（虚拟滚动容器）──────────────────────────────────── */
.pqs-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
  -webkit-overflow-scrolling: touch;
  padding: var(--space-2, 8px) 0;
  position: relative;
}

/* 可视区域：撑开真实滚动条高度 */
.pqs-viewport {
  position: relative;
  width: 100%;
}

/* ── 列表项 ────────────────────────────────────────────────── */
.pqs-item {
  display: grid;
  grid-template-columns: 40px 48px 1fr 56px;
  gap: var(--space-3, 12px);
  align-items: center;
  padding: var(--space-2, 8px) var(--space-4, 16px);
  cursor: pointer;
  transition: background 0.15s ease;
  position: relative;
  user-select: none;
}

/* 虚拟列表项：绝对定位 + 显式高度 + 性能隔离 */
.pqs-item--virtual {
  position: absolute;
  left: 0;
  right: 0;
  height: 56px;
  contain: layout style paint;
  will-change: transform;
}

.pqs-item:hover {
  background: var(--bg-hover, rgba(255, 255, 255, 0.05));
}

.pqs-item--current {
  background: var(--bg-active, rgba(255, 55, 95, 0.08));
}

/* 当前播放项左侧强调条 */
.pqs-item--current::before {
  content: '';
  position: absolute;
  left: 0;
  top: 8px;
  bottom: 8px;
  width: 3px;
  border-radius: 0 2px 2px 0;
  background: var(--accent-primary, #ff375f);
}

.pqs-item--current:hover {
  background: var(--bg-active, rgba(255, 55, 95, 0.12));
}

/* ── 序号列 ────────────────────────────────────────────────── */
.pqs-col-number {
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: var(--font-size-sm, 13px);
  color: var(--text-tertiary, #999);
  font-variant-numeric: tabular-nums;
}

.pqs-item--current .pqs-col-number {
  color: var(--accent-primary, #ff375f);
}

/* ── 播放中的波形图标 ──────────────────────────────────────── */
.pqs-playing-icon {
  display: inline-flex;
  align-items: flex-end;
  gap: 2px;
  height: 14px;
}

.pqs-playing-icon .bar {
  display: block;
  width: 2px;
  height: 6px;
  background: var(--accent-primary, #ff375f);
  border-radius: 1px;
  transform-origin: bottom;
}

.pqs-playing-icon.is-playing .bar {
  animation: pqs-bar-bounce 0.9s ease-in-out infinite;
}

.pqs-playing-icon.is-playing .bar:nth-child(2) {
  animation-delay: 0.15s;
}

.pqs-playing-icon.is-playing .bar:nth-child(3) {
  animation-delay: 0.3s;
}

@keyframes pqs-bar-bounce {
  0%, 100% { height: 4px; }
  50% { height: 12px; }
}

/* ── 封面 ──────────────────────────────────────────────────── */
.pqs-cover {
  width: 40px;
  height: 40px;
  border-radius: var(--radius-sm, 6px);
  overflow: hidden;
  background: var(--bg-tertiary, rgba(255, 255, 255, 0.05));
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.pqs-cover img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.pqs-cover-placeholder {
  font-size: 18px;
  color: var(--text-tertiary, #999);
}

/* ── 标题信息 ──────────────────────────────────────────────── */
.pqs-info {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.pqs-title {
  font-size: var(--font-size-md, 14px);
  color: var(--text-primary, #fff);
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.pqs-item--current .pqs-title {
  color: var(--accent-primary, #ff375f);
}

.pqs-artist {
  font-size: var(--font-size-xs, 12px);
  color: var(--text-secondary, #999);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* ── 时长 ──────────────────────────────────────────────────── */
.pqs-duration {
  font-size: var(--font-size-xs, 12px);
  color: var(--text-tertiary, #999);
  text-align: right;
  font-variant-numeric: tabular-nums;
}

/* ── 移动端适配 ────────────────────────────────────────────── */
@media (max-width: 768px) {
  .pqs-item {
    grid-template-columns: 32px 40px 1fr 48px;
    gap: var(--space-2, 8px);
    padding: var(--space-2, 8px) var(--space-3, 12px);
  }

  .pqs-cover {
    width: 40px;
    height: 40px;
  }

  .pqs-header {
    padding: var(--space-2, 8px) var(--space-3, 12px) var(--space-2, 8px);
  }
}

/* ── 暗黑模式适配（默认已是暗色，此处仅微调）──────────────── */
@media (prefers-color-scheme: light) {
  .pqs-empty {
    color: var(--text-tertiary, #666);
  }
}
</style>
