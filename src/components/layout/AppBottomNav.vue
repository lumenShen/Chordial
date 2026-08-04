<script setup>
import { useRoute, useRouter } from 'vue-router';
import { computed, onMounted, useTemplateRef, watch, nextTick } from 'vue';
import { HugeiconsIcon } from '@hugeicons/vue';
import {
  Home01Icon,
  HeartIcon,
  MusicNote01Icon,
  UserGroupIcon,
  DiscIcon,
  Playlist01Icon,
} from '@hugeicons/core-free-icons';
import PlayerStore from '@/stores/player/index.js';
import { useAnime } from '@/composables/useAnime.js';
import { ANIME_SPRINGS } from '@/utils/animePresets.js';
import LiquidGlass from '@/components/ui/LiquidGlass.vue';
import LiquidGlassFilter from '@/components/ui/LiquidGlassFilter.vue';
import { useLiquidGlass } from '@/composables/useLiquidGlass.js';
const route = useRoute();
const router = useRouter();

const hasCurrentTrack = computed(() => PlayerStore.hasCurrentTrack.value);

const rootRef = useTemplateRef('root');
const indicatorRef = useTemplateRef('indicator');
const { run, animate } = useAnime(() => rootRef.value);
const {
  glassStyle: indicatorGlassStyle,
  filterProps: indicatorFilterProps,
} = useLiquidGlass(() => ({
  refractionMode: 'standard',
  displacementScale: 80,
  blurAmount: 0.12,
  saturation: 160,
  aberration: 1,
  elasticity: 0,
  borderRadius: '999px',
  interactive: false,
}));

const navItems = [
  { path: '/', name: '首页', icon: Home01Icon },
  { path: '/favorites', name: '偏好库', icon: HeartIcon },
  { path: '/tracks', name: '歌曲', icon: MusicNote01Icon },
  { path: '/artists', name: '歌手', icon: UserGroupIcon },
  { path: '/albums', name: '专辑', icon: DiscIcon },
  { path: '/playlists', name: '歌单', icon: Playlist01Icon },
];

const isActive = (path) => {
  if (path === '/') return route.path === '/';
  return route.path.startsWith(path);
};


const getCurrentIndexFromRoute = () => {
  const idx = navItems.findIndex((item) => isActive(item.path));
  return idx >= 0 ? idx : 0;
};
const MAX_DEFORM_PX = 22;
const DEFORM_THRESHOLD_PX = 6;
const MAX_VELOCITY_DEFORM = 22;
const VELOCITY_THRESHOLD = 0.5;
const VELOCITY_NORMALIZE_BASE = 25;
const PAUSE_DURATION_MS = 80;
const PAUSE_VELOCITY_THRESHOLD = 1.5;
const SMOOTHING_FACTOR = 0.3;
const BOUNDARY_DAMPING = 0.3;
const DRAG_START_THRESHOLD = 8;

let lastIndicatorX = 0;
let lastIndicatorW = 0;
let isIndicatorMoving = false;
let currentAnimation = null;

let isDragging = false;
let potentialDrag = false;
let suppressClick = false;
let pointerStartX = 0;
let pointerStartY = 0;
let lastPointerX = 0;
let lastTimestamp = 0;
let dragOffsetX = 0;
let dragStartIndex = 0;
let baseItemWidth = 0;
let smoothedVelocity = 0;
let initialDragDirection = 0;
let lastActiveTimestamp = 0;
let pauseDetected = false;
let activePointerId = null;
function getAnchorPositions() {
  const root = rootRef.value;
  if (!root) return [];
  const items = root.querySelectorAll('.bottom-nav-item');
  const navRect = root.getBoundingClientRect();
  return Array.from(items).map((item) => {
    const rect = item.getBoundingClientRect();
    return rect.left - navRect.left;
  });
}

function getSlotWidth() {
  const positions = getAnchorPositions();
  if (positions.length < 2) return 0;
  return positions[1] - positions[0];
}

function computeVelocityDeform() {
  const absVelocity = Math.abs(smoothedVelocity);
  const now = Date.now();

  if (absVelocity >= PAUSE_VELOCITY_THRESHOLD) {
    lastActiveTimestamp = now;
  }
  if (lastActiveTimestamp > 0 && absVelocity < PAUSE_VELOCITY_THRESHOLD) {
    if (now - lastActiveTimestamp >= PAUSE_DURATION_MS) {
      pauseDetected = true;
      initialDragDirection = 0;
      lastActiveTimestamp = 0;
    }
  }

  if (absVelocity < VELOCITY_THRESHOLD) return 0;

  if (initialDragDirection === 0 || pauseDetected) {
    initialDragDirection = smoothedVelocity > 0 ? 1 : -1;
    pauseDetected = false;
  }

  const normalized = Math.min(absVelocity / VELOCITY_NORMALIZE_BASE, 1);
  const eased = Math.sqrt(normalized);
  const deform = eased * MAX_VELOCITY_DEFORM;
  const currentDir = smoothedVelocity > 0 ? 1 : -1;
  return currentDir === initialDragDirection ? deform : -deform * 0.5;
}
function applyDragTransform() {
  const indicator = indicatorRef.value;
  const positions = getAnchorPositions();
  if (!indicator || positions.length === 0) return;

  const baseX = positions[dragStartIndex] ?? 0;
  const deformAmount = computeVelocityDeform();
  const x = baseX + dragOffsetX;
  const w = Math.max(baseItemWidth + deformAmount, 0);

  indicator.style.transform = `translateX(${x}px)`;
  indicator.style.width = `${w}px`;

  lastIndicatorX = x;
  lastIndicatorW = w;
}

function onPointerDown(e) {
  if (e.button !== undefined && e.button !== 0) return;
  potentialDrag = true;
  pointerStartX = e.clientX;
  pointerStartY = e.clientY;
  lastPointerX = e.clientX;
  lastTimestamp = Date.now();
}

function onPointerMove(e) {
  if (!potentialDrag && !isDragging) return;

  if (potentialDrag && !isDragging) {
    const dx = e.clientX - pointerStartX;
    const dy = e.clientY - pointerStartY;
    if (Math.abs(dx) > DRAG_START_THRESHOLD && Math.abs(dx) > Math.abs(dy)) {
      startDrag(e);
    } else {
      return;
    }
  }

  if (!isDragging) return;

  const now = Date.now();
  const moveDx = e.clientX - lastPointerX;
  const dt = Math.max(now - lastTimestamp, 1);
  const velocity = moveDx / (dt / 16);
  smoothedVelocity = smoothedVelocity * (1 - SMOOTHING_FACTOR) + velocity * SMOOTHING_FACTOR;
  lastPointerX = e.clientX;
  lastTimestamp = now;

  const totalDx = e.clientX - pointerStartX;
  if (dragStartIndex === 0 && totalDx < 0) {
    dragOffsetX = totalDx * BOUNDARY_DAMPING;
  } else if (dragStartIndex === navItems.length - 1 && totalDx > 0) {
    dragOffsetX = totalDx * BOUNDARY_DAMPING;
  } else {
    dragOffsetX = totalDx;
  }

  applyDragTransform();
}
function startDrag(e) {
  isDragging = true;
  potentialDrag = false;
  dragStartIndex = getCurrentIndexFromRoute();
  const items = rootRef.value?.querySelectorAll('.bottom-nav-item');
  baseItemWidth = items?.[dragStartIndex]?.getBoundingClientRect().width ?? 0;
  dragOffsetX = 0;
  smoothedVelocity = 0;
  initialDragDirection = 0;
  pauseDetected = false;
  lastActiveTimestamp = 0;

  if (currentAnimation) {
    currentAnimation.pause();
    currentAnimation = null;
  }
  indicatorRef.value?.classList.add('moving');
  isIndicatorMoving = true;

  activePointerId = e.pointerId;
  if (rootRef.value && e.pointerId !== undefined) {
    try { rootRef.value.setPointerCapture(e.pointerId); } catch (_) { /* ignore */ }
  }
}

function endDrag() {
  if (!isDragging) return;
  isDragging = false;

  if (activePointerId !== null && rootRef.value) {
    try { rootRef.value.releasePointerCapture(activePointerId); } catch (_) { /* ignore */ }
    activePointerId = null;
  }

  const slotWidth = getSlotWidth();
  const movedSlots = slotWidth > 0 ? Math.round(dragOffsetX / slotWidth) : 0;
  let newIndex = dragStartIndex + movedSlots;
  newIndex = Math.max(0, Math.min(navItems.length - 1, newIndex));

  suppressClick = true;
  setTimeout(() => { suppressClick = false; }, 120);

  if (newIndex !== dragStartIndex) {
    router.push(navItems[newIndex].path);
  } else {
    updateIndicator();
  }
}
function onPointerUp() {
  potentialDrag = false;
  if (isDragging) endDrag();
}

function onPointerCancel() {
  potentialDrag = false;
  if (isDragging) {
    isDragging = false;
    if (activePointerId !== null && rootRef.value) {
      try { rootRef.value.releasePointerCapture(activePointerId); } catch (_) { /* ignore */ }
      activePointerId = null;
    }
    updateIndicator();
  }
}

function onRootClickCapture(e) {
  if (suppressClick) {
    e.preventDefault();
    e.stopPropagation();
  }
}
function updateIndicator() {
  if (isDragging) return;

  const root = rootRef.value;
  const indicator = indicatorRef.value;
  if (!root || !indicator) return;

  if (currentAnimation) {
    currentAnimation.pause();
    currentAnimation = null;
  }
  indicator.classList.remove('moving');
  isIndicatorMoving = false;

  const activeEl = root.querySelector('.bottom-nav-item.active');
  if (!activeEl) {
    currentAnimation = animate(indicator, { opacity: 0, duration: 200, ease: ANIME_SPRINGS.sensitive });
    return;
  }
  const navRect = root.getBoundingClientRect();
  const itemRect = activeEl.getBoundingClientRect();
  const targetX = itemRect.left - navRect.left;
  const targetW = itemRect.width;

  const isFirstRender = lastIndicatorW === 0;
  if (isFirstRender) {
    currentAnimation = animate(indicator, {
      translateX: targetX,
      width: targetW,
      opacity: 1,
      duration: 420,
      ease: ANIME_SPRINGS.bouncy,
    });
    lastIndicatorX = targetX;
    lastIndicatorW = targetW;
    return;
  }

  const distance = Math.abs(targetX - lastIndicatorX);
  const normalizedDist = navRect.width > 0 ? Math.min(distance / navRect.width, 1) : 0;
  const easedDist = Math.sqrt(normalizedDist);
  const deformAmount = distance < DEFORM_THRESHOLD_PX ? 0 : easedDist * MAX_DEFORM_PX;
  if (deformAmount > 0) {
    indicator.classList.add('moving');
    isIndicatorMoving = true;
    currentAnimation = animate(indicator, {
      translateX: targetX,
      width: [
        { value: targetW + deformAmount, duration: 150 },
        { value: targetW, duration: 400 },
      ],
      opacity: 1,
      duration: 550,
      ease: ANIME_SPRINGS.bouncy,
      onComplete: () => {
        indicator.classList.remove('moving');
        isIndicatorMoving = false;
        currentAnimation = null;
      },
    });
  } else {
    currentAnimation = animate(indicator, {
      translateX: targetX,
      width: targetW,
      opacity: 1,
      duration: 320,
      ease: ANIME_SPRINGS.bouncy,
      onComplete: () => {
        indicator.classList.remove('moving');
        isIndicatorMoving = false;
        currentAnimation = null;
      },
    });
  }

  lastIndicatorX = targetX;
  lastIndicatorW = targetW;
}

onMounted(() => {
  run(({ animate, stagger, presets }) => {
    animate('.bottom-nav-item', {
      ...presets.fadeInUp,
      delay: stagger(50),
      onComplete: () => {
        rootRef.value?.querySelectorAll('.bottom-nav-item').forEach((el) => {
          el.style.transform = '';
        });
      },
    });
  });
  nextTick(updateIndicator);
});

watch(() => route.path, () => {
  nextTick(updateIndicator);
});
</script>
<template>
  <div
    ref="root"
    class="app-bottom-nav"
    :class="{ 'with-player': hasCurrentTrack }"
    @pointerdown="onPointerDown"
    @pointermove="onPointerMove"
    @pointerup="onPointerUp"
    @pointercancel="onPointerCancel"
    @click.capture="onRootClickCapture"
  >
    <LiquidGlass :corner-radius="999" :elasticity="0.35" padding="0 16px" extra-class="nav-glass">
      <div ref="indicator" class="nav-indicator" :style="indicatorGlassStyle">
        <LiquidGlassFilter v-bind="indicatorFilterProps" />
      </div>
        <router-link
          v-for="item in navItems"
          :key="item.path"
          :to="item.path"
          class="bottom-nav-item"
          :class="{ active: isActive(item.path) }"
        >
          <HugeiconsIcon :icon="item.icon" :size="20" class="bottom-nav-icon" />
          <span class="bottom-nav-text">{{ item.name }}</span>
        </router-link>
    </LiquidGlass>
  </div>
</template>
<style scoped>
.app-bottom-nav {
  position: fixed;
  bottom: 1.5rem;
  left: 1.5rem;
  z-index: 100;
  touch-action: pan-y;
  transition: bottom var(--transition-slow);
}

.nav-glass {
  height: var(--bottom-nav-height);
  display: flex;
  align-items: center;
  gap: 4px;
  width: fit-content;
  max-width: calc(100vw - 3rem);
  padding-bottom: var(--safe-area-bottom);
  overflow: hidden;
  isolation: isolate;
  user-select: none;
  -webkit-user-select: none;
}

.nav-indicator {
  position: absolute;
  left: 0;
  top: calc(var(--bottom-nav-height) * 0.14);
  height: calc(var(--bottom-nav-height) * 0.72);
  width: 0;
  opacity: 0;
  z-index: 0;
  pointer-events: none;
  will-change: transform, width, backdrop-filter;
  background: color-mix(in srgb, var(--primary-color) 22%, transparent);
  box-shadow: 0 2px 8px color-mix(in srgb, var(--primary-color) 12%, transparent);
  transition: background 220ms ease, box-shadow 220ms ease;
}

.nav-indicator.moving {
  background: color-mix(in srgb, var(--primary-color) 34%, transparent);
  box-shadow:
    0 4px 16px color-mix(in srgb, var(--primary-color) 28%, transparent),
    inset 0 1px 0 rgba(255, 255, 255, 0.25);
}
.bottom-nav-item {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 0 18px;
  color: var(--text-secondary);
  text-decoration: none;
  transition: color var(--transition-fast);
  border-radius: var(--radius-md);
  flex: 0 0 auto;
  font-weight: 500;
  min-width: 0;
  position: relative;
  z-index: 1;
}

.bottom-nav-item:hover {
  color: var(--text-primary);
}

.bottom-nav-item.active {
  color: var(--primary-color);
  text-shadow: var(--primary-color) 0 0 2em;
}

.bottom-nav-icon {
  font-size: 22px;
}

.bottom-nav-text {
  font-size: 11px;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

@media (max-width: 767px) {
  .app-bottom-nav {
    right: 1rem;
    left: 1rem;
    transform: none;
    bottom: var(--player-bar-height, 100px);
  }

  .nav-glass {
    width: auto;
  }
}

@media (prefers-color-scheme: dark) {
  .bottom-nav-item.active {
    color: var(--primary-color);
  }

  .nav-indicator {
    background: color-mix(in srgb, var(--primary-color) 30%, transparent);
  }

  .nav-indicator.moving {
    background: color-mix(in srgb, var(--primary-color) 42%, transparent);
  }
}
</style>