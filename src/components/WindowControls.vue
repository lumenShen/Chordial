<script setup>
import { ref, onMounted } from 'vue';
import {
  toggleAlwaysOnTop,
  closeWindow,
  minimizeWindow,
  toggleMaximize,
} from '@/api/window.js';
import { useLiquidGlass } from '@/composables/useLiquidGlass.js';
import LiquidGlassFilter from '@/components/ui/LiquidGlassFilter.vue';

const emit = defineEmits(['toggle-always-on-top', 'close', 'minimize', 'maximize']);

const isAlwaysOnTop = ref(false);
const isMaximized = ref(false);

// 液态玻璃：所有窗口控制按钮共享同一 filter
const {
  glassStyle,
  onEnter,
  onLeave,
  filterProps,
} = useLiquidGlass({
  blur: 10,
  scale: 0.012,
  hoverScale: 0.02,
  interactive: true,
  borderRadius: '50%',
});

async function handleToggleAlwaysOnTop() {
  isAlwaysOnTop.value = await toggleAlwaysOnTop();
  emit('toggle-always-on-top', isAlwaysOnTop.value);
}

async function handleCloseWindow() {
  await closeWindow();
  emit('close');
}

async function handleMinimizeWindow() {
  await minimizeWindow();
  emit('minimize');
}

async function handleToggleMaximize() {
  isMaximized.value = await toggleMaximize();
  emit('maximize', isMaximized.value);
}

onMounted(async () => {
  // 可以在这里获取初始窗口状态
});
</script>

<template>
  <div class="window-controls">
    <!-- 分隔线 -->
    <div class="divider"></div>

    <!-- 最小化按钮 -->
    <button
      class="window-btn"
      aria-label="最小化窗口"
      title="最小化"
      :style="glassStyle"
      @click="handleMinimizeWindow"
      @mouseenter="onEnter"
      @mouseleave="onLeave"
    >
      <i class="bi bi-dash-lg"></i>
    </button>

    <!-- 最大化/还原按钮 -->
    <button
      class="window-btn"
      aria-label="最大化/还原窗口"
      :title="isMaximized ? '还原' : '最大化'"
      :style="glassStyle"
      @click="handleToggleMaximize"
      @mouseenter="onEnter"
      @mouseleave="onLeave"
    >
      <i :class="isMaximized ? 'bi bi-fullscreen-exit' : 'bi bi-fullscreen'"></i>
    </button>

    <!-- 关闭按钮 -->
    <button
      class="window-btn close-btn"
      aria-label="关闭窗口"
      title="关闭"
      :style="glassStyle"
      @click="handleCloseWindow"
      @mouseenter="onEnter"
      @mouseleave="onLeave"
    >
      <i class="bi bi-x-lg"></i>
    </button>

    <!-- 液态玻璃 SVG filter 定义 -->
    <LiquidGlassFilter v-bind="filterProps" />
  </div>
</template>

<style scoped>
.window-controls {
  display: flex;
  align-items: center;
  gap: 8px;
  -webkit-app-region: no-drag;
}

.divider {
  width: 1px;
  height: 20px;
  background: var(--border-light);
  margin: 0 4px;
}

.window-btn {
  width: 36px;
  height: 36px;
  border: 1px solid rgba(255, 255, 255, 0.35);
  border-radius: 50%;
  /* 液态玻璃底色：backdrop-filter 由 composable inline style 注入 */
  background: rgba(255, 255, 255, 0.22);
  color: var(--text-secondary);
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  transition: color 0.2s ease, transform 0.2s ease;
  -webkit-app-region: no-drag;
  padding: 0;
  font: inherit;
  will-change: backdrop-filter;
  contain: paint;
}

.window-btn:hover {
  color: var(--text-primary);
}

.window-btn:active {
  transform: scale(0.95);
}

.window-btn.active {
  background-color: #0078d7;
  color: #fff;
}

.window-btn.active:hover {
  background-color: #006cbd;
}

.window-btn i {
  font-size: 14px;
}

.close-btn:hover {
  background-color: #e81123 !important;
  color: #fff !important;
}

/* 深色模式适配 */
@media (prefers-color-scheme: dark) {
  .window-btn {
    background: rgba(40, 40, 45, 0.5);
    border-color: rgba(255, 255, 255, 0.15);
    color: var(--text-secondary);
  }

  .window-btn:hover {
    color: var(--text-primary);
  }

  .divider {
    background: var(--border-light);
  }
}
</style>
