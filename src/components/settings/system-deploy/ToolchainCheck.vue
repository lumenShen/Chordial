<template>
  <div class="settings-section">
    <h3 class="block-title">
      <HIcon name="tools" />
      工具链检测
      <button class="refresh-btn" @click="onRefresh" :disabled="loading" title="重新检测">
          <HIcon name="arrow-clockwise" :class="{ spinning: loading }" />
      </button>
    </h3>

    <div v-if="!toolchain" class="stats-loading">检测中…</div>
    <div v-else class="toolchain-grid">
      <div class="toolchain-item" :class="{ ok: toolchain.git_available, bad: !toolchain.git_available }">
        <i class="bi" :class="toolchain.git_available ? 'bi-check-circle-fill' : 'bi-x-circle-fill'"></i>
        <div class="toolchain-info">
          <div class="toolchain-name">git</div>
          <div class="toolchain-version">{{ toolchain.git_version || '未检测到' }}</div>
        </div>
      </div>
      <div class="toolchain-item" :class="{ ok: toolchain.node_available, bad: !toolchain.node_available }">
        <i class="bi" :class="toolchain.node_available ? 'bi-check-circle-fill' : 'bi-x-circle-fill'"></i>
        <div class="toolchain-info">
          <div class="toolchain-name">node</div>
          <div class="toolchain-version">{{ toolchain.node_version || '未检测到' }}</div>
        </div>
      </div>
      <div class="toolchain-item" :class="{ ok: toolchain.npm_available, bad: !toolchain.npm_available }">
        <i class="bi" :class="toolchain.npm_available ? 'bi-check-circle-fill' : 'bi-x-circle-fill'"></i>
        <div class="toolchain-info">
          <div class="toolchain-name">npm</div>
          <div class="toolchain-version">{{ toolchain.npm_version || '未检测到' }}</div>
        </div>
      </div>
    </div>

    <div v-if="toolchain && !allToolsAvailable" class="toolchain-warn">
      <HIcon name="exclamation-triangle-fill" />
      缺少必要工具，请先安装 <a href="https://git-scm.com/downloads" target="_blank" rel="noopener">Git</a> 与
      <a href="https://nodejs.org/" target="_blank" rel="noopener">Node.js</a>（含 npm）后重启应用。
    </div>
  </div>
</template>

<script setup>
import HIcon from '@/components/ui/HIcon.vue';

defineProps({
  toolchain: { type: Object, default: null },
  loading: { type: Boolean, default: false },
  allToolsAvailable: { type: Boolean, default: false },
});

const emit = defineEmits(['refresh']);
function onRefresh() {
  emit('refresh');
}
</script>

<style scoped>
.refresh-btn {
  margin-left: auto;
  background: transparent;
  border: none;
  color: var(--text-secondary, #666);
  cursor: pointer;
  padding: 4px 8px;
  border-radius: 6px;
  font-size: 14px;
  transition: all 0.2s ease;
}

.refresh-btn:hover:not(:disabled) {
  background: var(--bg-hover, rgba(0, 0, 0, 0.04));
  color: var(--primary-color);
}

.refresh-btn:disabled {
  cursor: not-allowed;
  opacity: 0.5;
}

.spinning {
  display: inline-block;
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}

.stats-loading {
  padding: 24px 0;
  text-align: center;
  color: var(--text-secondary, #666);
  font-size: 13px;
}

/* 工具链 */
.toolchain-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 12px;
  margin-bottom: 12px;
}

.toolchain-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 12px;
  border-radius: 10px;
  background: var(--bg-tertiary, rgba(0, 0, 0, 0.03));
  border: 1px solid var(--border-light, #e8e8e8);
}

.toolchain-item.ok {
  border-color: rgba(40, 167, 69, 0.3);
}

.toolchain-item.bad {
  border-color: rgba(220, 53, 69, 0.3);
}

.toolchain-item i {
  font-size: 18px;
}

.toolchain-item.ok i {
  color: #28a745;
}

.toolchain-item.bad i {
  color: #dc3545;
}

.toolchain-info {
  min-width: 0;
}

.toolchain-name {
  font-weight: 600;
  font-size: 13px;
  color: var(--text-primary, #333);
}

.toolchain-version {
  font-size: 12px;
  color: var(--text-secondary, #666);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.toolchain-warn {
  margin-top: 12px;
  padding: 10px 12px;
  border-radius: 8px;
  background: rgba(255, 193, 7, 0.12);
  color: #856404;
  font-size: 13px;
  display: flex;
  align-items: center;
  gap: 8px;
}

.toolchain-warn a {
  color: #856404;
  text-decoration: underline;
}
</style>