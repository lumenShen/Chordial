<template>
  <div class="settings-section">
    <h3 class="block-title"><HIcon name="power" />服务</h3>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">启用共享服务</label>
        <span class="setting-desc">启动 P2P 服务器，允许其他 Chordial 实例连接</span>
      </div>
      <div class="setting-control">
        <label class="toggle">
          <input type="checkbox" v-model="p2pEnabled" @change="onToggleP2p" />
          <span class="toggle-slider"></span>
        </label>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">允许广播发现</label>
        <span class="setting-desc">局域网内其他 Chordial 可发现本机；关闭后只能通过 IP:端口+匹配码连接</span>
      </div>
      <div class="setting-control">
        <label class="toggle">
          <input type="checkbox" v-model="p2pBroadcast" @change="onToggleBroadcast" :disabled="!p2pEnabled" />
          <span class="toggle-slider"></span>
        </label>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">本机共享权限</label>
        <span class="setting-desc">决定对方拿到本库的权限；默认只读</span>
      </div>
      <div class="setting-control">
        <select v-model="p2pPermission" @change="onPermissionChange" :disabled="!p2pEnabled" class="select">
          <option value="readonly">仅可读</option>
          <option value="editable">可编辑</option>
        </select>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">本机监听地址</label>
        <span class="setting-desc">将此地址告知对方以建立连接</span>
      </div>
      <div class="setting-control">
        <code v-if="p2pStatus.listening" class="match-code">{{ p2pStatus.listen_addr }}</code>
        <span v-else class="setting-value">未启动</span>
      </div>
    </div>
  </div>
</template>

<script setup>
import { useP2pSettings } from '@/composables/useP2pSettings.js';

const {
  p2pEnabled,
  p2pBroadcast,
  p2pPermission,
  p2pStatus,
  onToggleP2p,
  onToggleBroadcast,
  onPermissionChange,
} = useP2pSettings();
</script>

<style scoped>
.setting-value {
  font-size: 13px;
  color: var(--text-secondary, #666);
}

.match-code {
  font-family: ui-monospace, monospace;
  font-size: 14px;
  background: var(--bg-primary, #f5f5f5);
  border: 1px solid var(--border-color, #e8e8e8);
  padding: 6px 12px;
  border-radius: 6px;
  color: var(--text-primary, #333);
  letter-spacing: 1px;
}

.toggle input:disabled + .toggle-slider {
  opacity: 0.4;
  cursor: not-allowed;
}

.select {
  padding: 7px 28px 7px 10px;
  font-size: 13px;
  border: 1px solid var(--border-color, #e8e8e8);
  border-radius: 8px;
  background: var(--bg-primary, #fff);
  color: var(--text-primary, #333);
  cursor: pointer;
  outline: none;
  min-width: 110px;
  appearance: none;
  background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'%3E%3Cpath fill='%23666' d='M6 8L1 3h10z'/%3E%3C/svg%3E");
  background-repeat: no-repeat;
  background-position: right 10px center;
}

.select:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
</style>