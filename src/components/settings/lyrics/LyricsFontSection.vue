<template>
  <div class="settings-section">
    <h3 class="block-title">
      <HIcon name="type" />
      歌词字体
    </h3>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">字体族</label>
        <span class="setting-desc">留空使用默认字体；CSS font-family 语法</span>
      </div>
      <div class="setting-control">
        <input
          type="text"
          v-model="settings.lyricFontFamily"
          placeholder="例: 'PingFang SC', system-ui"
          class="text-input"
        />
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">字重</label>
        <span class="setting-desc">CSS font-weight</span>
      </div>
      <div class="setting-control">
        <select v-model.number="settings.lyricFontWeight" class="select">
          <option :value="400">Regular (400)</option>
          <option :value="500">Medium (500)</option>
          <option :value="600">Semibold (600)</option>
          <option :value="700">Bold (700)</option>
          <option :value="800">Heavy (800)</option>
        </select>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">字符间距</label>
        <span class="setting-desc">CSS letter-spacing</span>
      </div>
      <div class="setting-control">
        <select v-model="settings.lyricLetterSpacing" class="select">
          <option value="normal">默认</option>
          <option value="0.02em">疏松 (0.02em)</option>
          <option value="0.05em">较疏 (0.05em)</option>
          <option value="0.1em">稀疏 (0.1em)</option>
          <option value="-0.02em">紧凑 (-0.02em)</option>
        </select>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">字体大小</label>
        <span class="setting-desc">预设字号档位</span>
      </div>
      <div class="setting-control">
        <select v-model="settings.lyricSizePreset" class="select">
          <option value="tiny">极小</option>
          <option value="extra-small">特小</option>
          <option value="small">小</option>
          <option value="medium">中</option>
          <option value="large">大</option>
          <option value="extra-large">特大</option>
          <option value="huge">巨大</option>
        </select>
      </div>
    </div>
  </div>
</template>

<script setup>
import { useLyricsSettings } from '@/composables/useLyricsSettings.js';

const { settings } = useLyricsSettings();
</script>

<style scoped>
.select {
  padding: 7px 28px 7px 10px;
  font-size: 13px;
  border: 1px solid var(--border-color, #e8e8e8);
  border-radius: 8px;
  background: var(--bg-primary, #fff);
  color: var(--text-primary, #333);
  cursor: pointer;
  outline: none;
  min-width: 130px;
  appearance: none;
  background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'%3E%3Cpath fill='%23666' d='M6 8L1 3h10z'/%3E%3C/svg%3E");
  background-repeat: no-repeat;
  background-position: right 10px center;
}

.text-input {
  padding: 7px 10px;
  font-size: 13px;
  border: 1px solid var(--border-color, #e8e8e8);
  border-radius: 8px;
  background: var(--bg-primary, #fff);
  color: var(--text-primary, #333);
  outline: none;
  min-width: 200px;
  font-family: inherit;
}

@media (max-width: 767px) {
  .text-input,
  .select {
    min-width: 0;
    flex: 1;
  }
}
</style>