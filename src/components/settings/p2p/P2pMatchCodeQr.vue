<template>
  <div class="settings-section">
    <h3 class="block-title"><HIcon name="qr-code" />匹配码 / 二维码</h3>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">当前匹配码</label>
        <span class="setting-desc">
          6 位数字，每 60 秒自动轮换防撞码。
          <span v-if="p2pStatus.listening && rotateCountdown > 0" class="countdown">
            {{ rotateCountdown }}s 后更新
          </span>
        </span>
      </div>
      <div class="setting-control">
        <code v-if="p2pStatus.match_code" class="match-code big">{{ p2pStatus.match_code }}</code>
        <span v-else class="setting-value">未启动</span>
        <button class="action-btn" @click="onRegenerateCode" :disabled="!p2pEnabled">重新生成</button>
      </div>
    </div>

    <div v-if="p2pEnabled && qrDataUrl" class="qr-area">
      <div class="qr-side">
        <img :src="qrDataUrl" alt="匹配二维码" class="qr-img" />
        <p class="qr-tip">让对方扫描此二维码即可建立连接</p>
      </div>
      <div class="qr-side">
        <button class="action-btn primary" @click="openScanner">
          <HIcon name="camera" /> 扫描对方二维码
        </button>
        <p class="qr-tip">使用摄像头扫描对方展示的匹配二维码</p>
      </div>
    </div>
  </div>
</template>

<script setup>
import { useP2pSettings } from '@/composables/useP2pSettings.js';

const {
  p2pEnabled,
  p2pStatus,
  qrDataUrl,
  rotateCountdown,
  onRegenerateCode,
  openScanner,
} = useP2pSettings();
</script>

<style scoped>
.countdown {
  color: var(--primary-color);
  font-weight: 600;
}

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

.match-code.big {
  font-size: 20px;
  font-weight: 700;
  letter-spacing: 4px;
  padding: 8px 16px;
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

/* 二维码区 */
.qr-area {
  display: flex;
  gap: 24px;
  margin-top: 16px;
  padding: 16px;
  border-radius: 12px;
  background: var(--bg-primary, #f5f5f5);
  border: 1px dashed var(--border-color, #e8e8e8);
  flex-wrap: wrap;
}

.qr-side {
  flex: 1;
  min-width: 200px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
}

.qr-img {
  width: 200px;
  height: 200px;
  border-radius: 8px;
  background: #fff;
}

.qr-tip {
  font-size: 12px;
  color: var(--text-secondary, #666);
  margin: 0;
  text-align: center;
}

@media (max-width: 767px) {
  .qr-area {
    flex-direction: column;
  }
}
</style>