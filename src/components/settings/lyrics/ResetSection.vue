<template>
  <div class="settings-section">
    <h3 class="block-title">
      <HIcon name="arrow-counterclockwise" />
      重置
    </h3>
    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">恢复默认</label>
        <span class="setting-desc">清除所有 AMLL 配置，恢复出厂默认值</span>
      </div>
      <div class="setting-control">
        <button class="reset-btn" @click="resetToDefaults">恢复默认</button>
      </div>
    </div>
  </div>
</template>

<script setup>
import { useLyricsSettings } from '@/composables/useLyricsSettings.js';

const { resetToDefaults } = useLyricsSettings();
</script>

<style scoped>
.reset-btn {
  padding: 7px 14px;
  font-size: 13px;
  border: 1px solid var(--border-color, #e8e8e8);
  border-radius: 8px;
  background: var(--bg-primary, #fff);
  color: var(--text-primary, #333);
  cursor: pointer;
  transition: all 0.2s;
}

.reset-btn:hover {
  border-color: #e34a4a;
  color: #e34a4a;
}
</style>