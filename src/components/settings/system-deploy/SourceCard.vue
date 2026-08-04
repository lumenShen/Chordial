<template>
  <div class="settings-section">
    <h3 class="block-title">
      <i class="bi" :class="sourceIcon(src.spec.id)"></i>
      {{ src.spec.display_name }}
      <span class="status-badge" :class="statusClass(src.runtime.status)">
        {{ statusLabel(src.runtime.status) }}
      </span>
    </h3>

    <p class="source-desc">{{ src.spec.description }}</p>
    <p class="source-meta">
      <span class="meta-item"><HIcon name="github" /> <a :href="src.spec.git_url" target="_blank" rel="noopener">{{ src.spec.git_url }}</a></span>
      <span class="meta-item"><HIcon name="hdd-network" /> 端口 {{ effectivePort(src) }}</span>
      <span v-if="src.runtime.pid" class="meta-item"><HIcon name="cpu" /> PID {{ src.runtime.pid }}</span>
    </p>

    <div v-if="src.config.last_error" class="error-banner">
      <HIcon name="exclamation-triangle-fill" />
      <pre class="error-text">{{ src.config.last_error }}</pre>
    </div>

    <SourceConfigForm
      :src="src"
      @port-change="onPortChange($event)"
      @toggle-enabled="onToggleEnabled($event)"
      @toggle-auto-start="onToggleAutoStart($event)"
      @toggle-auto-base-url="onToggleAutoBaseUrl($event)"
    />

    <!-- 操作按钮 -->
    <div class="action-row">
      <button
        v-if="src.runtime.status === 'not_deployed'"
        class="btn btn-primary"
        :disabled="busy[src.spec.id] || !allToolsAvailable"
        @click="onDeployAndStart"
      >
        <i class="bi" :class="busy[src.spec.id] ? 'bi-hourglass-split' : 'bi-rocket-takeoff'"></i>
        {{ busy[src.spec.id] ? busy[src.spec.id] : '一键部署并启动' }}
      </button>

      <template v-else>
        <button
          v-if="src.runtime.status !== 'running'"
          class="btn btn-primary"
          :disabled="busy[src.spec.id] || !allToolsAvailable"
          @click="onStart"
        >
          <HIcon name="play-fill" /> 启动
        </button>
        <button
          v-else
          class="btn btn-secondary"
          :disabled="busy[src.spec.id]"
          @click="onStop"
        >
          <HIcon name="stop-fill" /> 停止
        </button>

        <button
          class="btn"
          :disabled="busy[src.spec.id] || src.runtime.status === 'running'"
          @click="onPull"
          title="拉取最新代码（git pull）"
        >
          <HIcon name="arrow-repeat" /> 更新代码
        </button>

        <button
          class="btn"
          :disabled="busy[src.spec.id] || src.runtime.status === 'running'"
          @click="onInstall"
          title="重新安装依赖（npm install）"
        >
          <HIcon name="box-seam" /> 重装依赖
        </button>

        <button
          class="btn btn-danger-ghost"
          :disabled="busy[src.spec.id]"
          @click="onRemove"
        >
          <HIcon name="trash" /> {{ pendingRemove === src.spec.id ? '确认删除' : '移除部署' }}
        </button>
      </template>
    </div>

    <!-- 日志展开 -->
    <SourceLogSection
      :src="src"
      :expanded="expandedLogs[src.spec.id]"
      @toggle="onToggleLogs"
    />
  </div>
</template>

<script setup>
import HIcon from '@/components/ui/HIcon.vue';
import SourceConfigForm from './SourceConfigForm.vue';
import SourceLogSection from './SourceLogSection.vue';

defineProps({
  src: { type: Object, required: true },
  busy: { type: Object, required: true },
  allToolsAvailable: { type: Boolean, default: false },
  expandedLogs: { type: Object, required: true },
  pendingRemove: { type: [String, Number], default: null },
  statusLabel: { type: Function, required: true },
  statusClass: { type: Function, required: true },
  sourceIcon: { type: Function, required: true },
  effectivePort: { type: Function, required: true },
});

const emit = defineEmits([
  'deploy-and-start',
  'start',
  'stop',
  'pull',
  'install',
  'remove',
  'port-change',
  'toggle-enabled',
  'toggle-auto-start',
  'toggle-auto-base-url',
  'toggle-logs',
]);

function onDeployAndStart() { emit('deploy-and-start'); }
function onStart() { emit('start'); }
function onStop() { emit('stop'); }
function onPull() { emit('pull'); }
function onInstall() { emit('install'); }
function onRemove() { emit('remove'); }
function onPortChange(ev) { emit('port-change', ev); }
function onToggleEnabled(ev) { emit('toggle-enabled', ev); }
function onToggleAutoStart(ev) { emit('toggle-auto-start', ev); }
function onToggleAutoBaseUrl(ev) { emit('toggle-auto-base-url', ev); }
function onToggleLogs() { emit('toggle-logs'); }
</script>

<style scoped>
/* 来源卡片 */
.source-desc {
  font-size: 13px;
  color: var(--text-secondary, #666);
  margin: 0 0 8px;
}

.source-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 16px;
  margin: 0 0 16px;
  font-size: 12px;
  color: var(--text-secondary, #666);
}

.source-meta a {
  color: var(--primary-color);
  text-decoration: none;
}

.source-meta a:hover {
  text-decoration: underline;
}

.meta-item {
  display: inline-flex;
  align-items: center;
  gap: 4px;
}

.status-badge {
  margin-left: auto;
  padding: 2px 10px;
  border-radius: 12px;
  font-size: 12px;
  font-weight: 600;
}

.status-badge.ok {
  background: rgba(40, 167, 69, 0.15);
  color: #28a745;
}

.status-badge.bad {
  background: rgba(220, 53, 69, 0.15);
  color: #dc3545;
}

.status-badge.info {
  background: rgba(0, 122, 255, 0.12);
  color: var(--primary-color);
}

.status-badge.neutral {
  background: rgba(0, 0, 0, 0.06);
  color: var(--text-secondary, #666);
}

.error-banner {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 10px 12px;
  border-radius: 8px;
  background: rgba(220, 53, 69, 0.1);
  color: #dc3545;
  font-size: 13px;
  margin-bottom: 12px;
}

.error-banner > i {
  flex-shrink: 0;
  margin-top: 2px;
}

.error-text {
  margin: 0;
  font-family: 'Consolas', 'Monaco', monospace;
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-all;
  flex: 1;
  min-width: 0;
}

/* 操作按钮 */
.action-row {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 16px;
}

.btn {
  padding: 7px 14px;
  border: 1px solid var(--border-light, #e8e8e8);
  border-radius: 8px;
  background: var(--bg-primary, #fff);
  color: var(--text-primary, #333);
  font-size: 13px;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  transition: all 0.15s ease;
}

.btn:hover:not(:disabled) {
  border-color: var(--primary-color);
  color: var(--primary-color);
}

.btn-primary {
  background: var(--primary-color);
  border-color: var(--primary-color);
  color: #fff;
}

.btn-primary:hover:not(:disabled) {
  filter: brightness(0.95);
  color: #fff;
}

.btn-secondary {
  background: var(--bg-secondary, #f5f5f5);
  border-color: var(--border-light, #e8e8e8);
}

.btn-danger-ghost {
  color: #dc3545;
  border-color: rgba(220, 53, 69, 0.3);
}

.btn-danger-ghost:hover:not(:disabled) {
  background: rgba(220, 53, 69, 0.08);
  color: #dc3545;
  border-color: #dc3545;
}
</style>