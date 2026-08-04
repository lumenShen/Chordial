<template>
  <div class="settings-section">
    <h3 class="block-title">
      <HIcon name="layout-text-window-reverse" />
      布局
    </h3>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">封面布局（隐藏歌词时）</label>
        <span class="setting-desc">控制隐藏歌词时专辑图的布局方式</span>
      </div>
      <div class="setting-control">
        <select v-model="settings.verticalCoverLayout" class="select">
          <option value="auto">自动（根据视频封面）</option>
          <option value="force-normal">强制普通</option>
          <option value="force-immersive">强制沉浸</option>
        </select>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">底部控件类型</label>
        <span class="setting-desc">歌曲信息下方的组件类型</span>
      </div>
      <div class="setting-control">
        <select v-model="settings.playerControlsType" class="select">
          <option value="controls">播放控制按钮</option>
          <option value="fft">音频可视化</option>
          <option value="none">不显示</option>
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

@media (max-width: 767px) {
  .select {
    min-width: 0;
    flex: 1;
  }
}
</style>