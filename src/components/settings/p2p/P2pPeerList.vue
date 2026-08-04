<template>
  <div v-if="disabledEmpty" class="empty-hint">先启用共享服务</div>
  <div v-else-if="!peers.length" class="empty-hint">{{ emptyText }}</div>
  <div v-else class="peer-list">
    <div v-for="(item, i) in peers" :key="rowKey(item, i)" class="peer-row">
      <div class="peer-info">
        <div class="peer-name">
          {{ item.name }}
          <template v-if="mode === 'discovered'">
            <span v-if="isTrusted(item)" class="badge trusted">已信任</span>
            <span v-else-if="isConnected(item)" class="badge connected">已连接</span>
          </template>
        </div>
        <div class="peer-meta">
          <template v-if="mode === 'discovered'">{{ item.addr }}:{{ item.port }}</template>
          <template v-else-if="mode === 'trusted'">
            {{ item.addr }} · {{ item.permission === 'editable' ? '可编辑' : '只读' }}
            · {{ formatTime(item.added_at) }}
          </template>
          <template v-else>
            {{ item.addr }} · {{ item.permission === 'editable' ? '可编辑' : '只读' }}
          </template>
        </div>
      </div>
      <div class="peer-actions">
        <template v-if="mode === 'discovered'">
          <button v-if="isTrusted(item)" class="action-btn ghost" disabled>自动连接</button>
          <button v-else-if="isConnected(item)" class="action-btn ghost" disabled>已连接</button>
          <button v-else class="action-btn primary" @click="onConnectDiscovered(item)">连接</button>
        </template>
        <template v-else-if="mode === 'trusted'">
          <button class="action-btn danger" @click="onRemoveTrusted(item.instance_id)">移除</button>
        </template>
        <template v-else>
          <button class="action-btn danger" @click="onDisconnect(item.id)">断开</button>
        </template>
      </div>
    </div>
  </div>
</template>

<script setup>
import { computed } from 'vue';
import { useP2pSettings } from '@/composables/useP2pSettings.js';

const props = defineProps({
  /** 'discovered' | 'trusted' | 'connected' */
  mode: { type: String, required: true },
  emptyText: { type: String, default: '' },
});

const {
  p2pEnabled,
  discoveredPeers,
  trustedDevices,
  connectedPeers,
  isTrusted,
  isConnected,
  peerKey,
  formatTime,
  onConnectDiscovered,
  onRemoveTrusted,
  onDisconnect,
} = useP2pSettings();

const peers = computed(() => {
  if (props.mode === 'discovered') return discoveredPeers.value;
  if (props.mode === 'trusted') return trustedDevices.value;
  return connectedPeers.value;
});

// 未启用共享服务时，局域网扫描区显示引导文案
const disabledEmpty = computed(
  () => props.mode === 'discovered' && !p2pEnabled.value,
);

function rowKey(item, index) {
  if (props.mode === 'discovered') return peerKey(item);
  if (props.mode === 'trusted') return item.instance_id;
  return item.id ?? index;
}
</script>

<style scoped>
/* 列表 */
.peer-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.peer-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px;
  border-radius: 10px;
  background: var(--bg-primary, #f5f5f5);
  border: 1px solid var(--border-color, #e8e8e8);
}

.peer-info {
  flex: 1;
  min-width: 0;
}

.peer-name {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary, #333);
  display: flex;
  align-items: center;
  gap: 8px;
}

.peer-meta {
  font-size: 12px;
  color: var(--text-secondary, #666);
  margin-top: 2px;
}

.peer-actions {
  flex-shrink: 0;
}

.badge {
  font-size: 10px;
  padding: 2px 8px;
  border-radius: 10px;
  font-weight: 600;
}

.badge.trusted {
  background: rgba(40, 167, 69, 0.15);
  color: #28a745;
}

.badge.connected {
  background: rgba(0, 122, 255, 0.15);
  color: var(--primary-color);
}

.empty-hint {
  font-size: 13px;
  color: var(--text-tertiary, #999);
  padding: 16px 0;
  text-align: center;
}

/* 本页 action-btn 为实心灰底变体，与全局线框版不同 */
.action-btn {
  padding: 7px 14px;
  border: 1px solid transparent;
  border-radius: 8px;
  background: var(--bg-hover, #eaeaea);
  color: var(--text-primary, #333);
  font-size: 13px;
  cursor: pointer;
  transition: all 0.2s ease;
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.action-btn:hover:not(:disabled) {
  background: var(--bg-active, #dcdcdc);
}

.action-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.action-btn.primary {
  background: var(--primary-color);
  color: #fff;
}

.action-btn.primary:hover:not(:disabled) {
  background: var(--primary-color-dark, #005a9e);
}

.action-btn.ghost {
  background: transparent;
  border-color: var(--border-color, #e8e8e8);
}

.action-btn.danger {
  background: #dc3545;
  color: #fff;
}

.action-btn.danger:hover:not(:disabled) {
  background: #c82333;
}
</style>