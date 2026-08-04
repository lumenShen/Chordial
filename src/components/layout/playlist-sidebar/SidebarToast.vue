<script setup>
/**
 * SidebarToast - 侧边栏轻提示
 *
 * 保留 toast 的进入/离开过渡（toast-pop），DOM 生命周期完整。
 */
defineProps({
  toast: { type: String, default: '' },
});
</script>

<template>
  <transition name="toast-pop">
    <div v-if="toast" class="sidebar-toast">{{ toast }}</div>
  </transition>
</template>

<style scoped>
/* ── 轻提示 ───────────────────────────────────────────── */
.sidebar-toast {
  position: absolute;
  left: 12px;
  right: 12px;
  bottom: 12px;
  padding: 8px 12px;
  background: var(--text-primary);
  color: var(--primary-contrast);
  font-size: var(--font-size-sm);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-md);
  z-index: 20;
}

.toast-pop-enter-active,
.toast-pop-leave-active {
  transition: opacity 0.2s, transform 0.2s;
}
.toast-pop-enter-from,
.toast-pop-leave-to {
  opacity: 0;
  transform: translateY(6px);
}
</style>