<template>
  <!-- Toast -->
  <transition name="fade">
    <div v-if="toast" class="toast" :class="toast.type">
      <i class="bi" :class="toastIcon"></i>
      <span>{{ toast.message }}</span>
    </div>
  </transition>
</template>

<script setup>
defineProps({
  toast: { type: Object, default: null },
  toastIcon: { type: String, default: '' },
});
</script>

<style scoped>
/* 本页 Toast 为实心背景变体，与全局线框版不同 */
.toast {
  position: fixed;
  bottom: 32px;
  left: 50%;
  transform: translateX(-50%);
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px 20px;
  border-radius: 12px;
  font-size: 14px;
  font-weight: 500;
  color: #fff;
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.15);
  z-index: 1000;
  pointer-events: none;
  max-width: 90vw;
}

.toast.success {
  background: #28a745;
}

.toast.error {
  background: #dc3545;
}

.toast.info {
  background: var(--primary-color);
}

.toast i {
  font-size: 16px;
}

.fade-enter-active,
.fade-leave-active {
  transition: all 0.3s ease;
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
  transform: translateX(-50%) translateY(12px);
}
</style>