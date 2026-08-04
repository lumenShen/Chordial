<template>
  <div class="logs-section" v-if="src.runtime.recent_logs && src.runtime.recent_logs.length > 0">
    <div class="logs-header" @click="onToggle">
      <i class="bi" :class="expanded ? 'bi-chevron-down' : 'bi-chevron-right'"></i>
      <span>最近日志（{{ src.runtime.recent_logs.length }} 条）</span>
    </div>
    <pre v-if="expanded" class="logs-content">{{ src.runtime.recent_logs.join('\n') }}</pre>
  </div>
</template>

<script setup>
defineProps({
  src: { type: Object, required: true },
  expanded: { type: Boolean, default: false },
});

const emit = defineEmits(['toggle']);
function onToggle() {
  emit('toggle');
}
</script>

<style scoped>
/* 日志 */
.logs-section {
  margin-top: 16px;
  border-top: 1px solid var(--border-lightest, rgba(0, 0, 0, 0.04));
  padding-top: 12px;
}

.logs-header {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-secondary, #666);
  cursor: pointer;
  user-select: none;
}

.logs-header:hover {
  color: var(--primary-color);
}

.logs-content {
  margin: 8px 0 0;
  padding: 12px;
  background: var(--bg-tertiary, rgba(0, 0, 0, 0.04));
  border-radius: 8px;
  font-family: 'Consolas', 'Monaco', monospace;
  font-size: 11px;
  line-height: 1.5;
  color: var(--text-secondary, #666);
  max-height: 300px;
  overflow-y: auto;
  white-space: pre-wrap;
  word-break: break-all;
}
</style>