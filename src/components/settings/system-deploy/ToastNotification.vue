<template>
  <transition name="toast">
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
/* Toast（本页为较窄居中卡片变体） */
.toast {
  position: fixed;
  bottom: 32px;
  left: 50%;
  transform: translateX(-50%);
  padding: 12px 20px;
  border-radius: 10px;
  background: var(--bg-secondary, #fff);
  border: 1px solid var(--border-light, #e8e8e8);
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.12);
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 13px;
  z-index: 1000;
  max-width: 480px;
}

.toast.success {
  border-color: rgba(40, 167, 69, 0.3);
  color: #28a745;
}

.toast.error {
  border-color: rgba(220, 53, 69, 0.3);
  color: #dc3545;
}

.toast.info {
  color: var(--text-primary, #333);
}

.toast-enter-active,
.toast-leave-active {
  transition: all 0.25s ease;
}

.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translateX(-50%) translateY(8px);
}
</style>