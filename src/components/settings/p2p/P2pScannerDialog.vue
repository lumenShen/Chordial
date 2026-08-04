<template>
  <div v-if="showScanner" class="scanner-overlay" @click.self="closeScanner">
    <div class="scanner-box">
      <div class="scanner-head">
        <h4>扫描二维码</h4>
        <button class="action-btn ghost" @click="closeScanner"><HIcon name="x-lg" /></button>
      </div>
      <video ref="videoEl" class="scanner-video" autoplay playsinline muted></video>
      <div class="scanner-foot">
        <p v-if="scannerError" class="scanner-error">{{ scannerError }}</p>
        <p v-else class="scanner-hint">将对方二维码对准摄像头</p>
      </div>
    </div>
  </div>
</template>

<script setup>
import { useP2pSettings } from '@/composables/useP2pSettings.js';

const { showScanner, videoEl, scannerError, closeScanner } = useP2pSettings();
</script>

<style scoped>
/* 扫描对话框 */
.scanner-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.7);
  z-index: 1000;
  display: flex;
  align-items: center;
  justify-content: center;
}

.scanner-box {
  width: 90%;
  max-width: 480px;
  background: var(--bg-secondary, #fff);
  border-radius: 16px;
  overflow: hidden;
}

.scanner-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 12px 16px;
  border-bottom: 1px solid var(--border-light, #e8e8e8);
}

.scanner-head h4 {
  margin: 0;
  font-size: 15px;
  font-weight: 600;
  color: var(--text-primary, #333);
}

.scanner-video {
  width: 100%;
  display: block;
  background: #000;
  max-height: 60vh;
  object-fit: cover;
}

.scanner-foot {
  padding: 10px 16px;
}

.scanner-hint {
  font-size: 12px;
  color: var(--text-secondary, #666);
  margin: 0;
  text-align: center;
}

.scanner-error {
  font-size: 12px;
  color: #dc3545;
  margin: 0;
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

.action-btn.ghost {
  background: transparent;
  border-color: var(--border-color, #e8e8e8);
}
</style>