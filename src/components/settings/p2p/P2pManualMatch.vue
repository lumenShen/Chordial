<template>
  <div class="settings-section">
    <h3 class="block-title"><HIcon name="link-45deg" />手动匹配</h3>
    <div class="setting-item col">
      <div class="setting-info">
        <label class="setting-label">主动发起匹配</label>
        <span class="setting-desc">输入对方的 IP:端口 与匹配码以发起握手</span>
      </div>
      <div class="setting-control wrap">
        <input v-model="remoteAddr" placeholder="192.168.1.10:58008" class="text-input" :disabled="!p2pEnabled" />
        <input v-model="remoteCode" placeholder="匹配码" maxlength="6" class="text-input code-input" :disabled="!p2pEnabled" />
        <button class="action-btn primary" @click="onRequestMatch" :disabled="!p2pEnabled || !remoteAddr || !remoteCode">连接</button>
      </div>
    </div>
  </div>
</template>

<script setup>
import { useP2pSettings } from '@/composables/useP2pSettings.js';

const { p2pEnabled, remoteAddr, remoteCode, onRequestMatch } = useP2pSettings();
</script>

<style scoped>
.setting-item.col {
  flex-direction: column;
  align-items: flex-start;
  gap: 10px;
}

.setting-control.wrap {
  flex-wrap: wrap;
  width: 100%;
  justify-content: flex-end;
}

.text-input {
  padding: 7px 10px;
  font-size: 13px;
  border: 1px solid var(--border-color, #e8e8e8);
  border-radius: 8px;
  background: var(--bg-primary, #fff);
  color: var(--text-primary, #333);
  outline: none;
  min-width: 180px;
}

.text-input:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.code-input {
  min-width: 90px;
  font-family: ui-monospace, monospace;
  letter-spacing: 2px;
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
</style>