<script setup>
import { ref, onMounted } from 'vue';
import {
  toggleAlwaysOnTop,
  closeWindow,
  minimizeWindow,
  toggleMaximize,
} from '@/api/window.js';

const emit = defineEmits(['toggle-always-on-top', 'close', 'minimize', 'maximize']);

const isAlwaysOnTop = ref(false);
const isMaximized = ref(false);

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
  <!-- 位于 AppHeader 浮动胶囊内的窗口控制按钮 -->
  <div class="window-controls">
    <!-- 最小化按钮 -->
    <button
      class="window-btn"
      aria-label="最小化窗口"
      title="最小化"
      @click="handleMinimizeWindow"
    >
      <i class="bi bi-dash-lg"></i>
    </button>

    <!-- 最大化/还原按钮 -->
    <button
      class="window-btn"
      aria-label="最大化/还原窗口"
      :title="isMaximized ? '还原' : '最大化'"
      @click="handleToggleMaximize"
    >
      <i :class="isMaximized ? 'bi bi-fullscreen-exit' : 'bi bi-fullscreen'"></i>
    </button>

    <!-- 关闭按钮 -->
    <button
      class="window-btn close-btn"
      aria-label="关闭窗口"
      title="关闭"
      @click="handleCloseWindow"
    >
      <i class="bi bi-x-lg"></i>
    </button>
  </div>
</template>

<style scoped>
.window-controls {
  display: flex;
  align-items: center;
  gap: 8px;
  -webkit-app-region: no-drag;
}

.window-btn {
  width: 34px;
  height: 34px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  transition: color var(--transition-fast), transform var(--transition-fast), background var(--transition-fast);
  -webkit-app-region: no-drag;
  padding: 0;
  font: inherit;
  border-radius: 50%;
}

.window-btn:hover {
  color: var(--text-primary);
  background: rgba(127, 127, 127, 0.1);
}

.window-btn:active {
  transform: scale(0.92);
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
</style>
