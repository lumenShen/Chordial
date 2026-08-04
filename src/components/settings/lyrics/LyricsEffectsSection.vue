<template>
  <div class="settings-section">
    <h3 class="block-title">
      <HIcon name="music-note-list" />
      歌词效果
    </h3>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">歌词行模糊效果</label>
        <span class="setting-desc">非当前歌词行模糊处理，性能影响较高</span>
      </div>
      <div class="setting-control">
        <label class="toggle">
          <input type="checkbox" v-model="settings.enableLyricLineBlurEffect" />
          <span class="toggle-slider"></span>
        </label>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">歌词行缩放效果</label>
        <span class="setting-desc">当前歌词行放大，无性能影响</span>
      </div>
      <div class="setting-control">
        <label class="toggle">
          <input type="checkbox" v-model="settings.enableLyricLineScaleEffect" />
          <span class="toggle-slider"></span>
        </label>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">弹簧物理动画</label>
        <span class="setting-desc">歌词切换使用弹簧算法，性能影响较高；关闭后回退到 transition 过渡</span>
      </div>
      <div class="setting-control">
        <label class="toggle">
          <input type="checkbox" v-model="settings.enableLyricLineSpringAnimation" />
          <span class="toggle-slider"></span>
        </label>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">显示翻译歌词</label>
        <span class="setting-desc">在原文下方显示翻译</span>
      </div>
      <div class="setting-control">
        <label class="toggle">
          <input type="checkbox" v-model="settings.enableLyricTranslationLine" />
          <span class="toggle-slider"></span>
        </label>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">显示音译歌词</label>
        <span class="setting-desc">在原文下方显示罗马音译</span>
      </div>
      <div class="setting-control">
        <label class="toggle">
          <input type="checkbox" v-model="settings.enableLyricRomanLine" />
          <span class="toggle-slider"></span>
        </label>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">交换翻译/音译位置</label>
        <span class="setting-desc">音译显示在翻译之上</span>
      </div>
      <div class="setting-control">
        <label class="toggle">
          <input type="checkbox" v-model="settings.enableLyricSwapTransRomanLine" />
          <span class="toggle-slider"></span>
        </label>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">逐词渐变宽度</label>
        <span class="setting-desc">0 关闭，0.5 模拟 iPad，1 模拟 Android</span>
      </div>
      <div class="setting-control">
        <input
          type="range" min="0" max="1" step="0.05"
          v-model.number="settings.lyricWordFadeWidth"
          class="slider"
        />
        <span class="setting-value">{{ settings.lyricWordFadeWidth.toFixed(2) }}</span>
      </div>
    </div>
  </div>
</template>

<script setup>
import { useLyricsSettings } from '@/composables/useLyricsSettings.js';

const { settings } = useLyricsSettings();
</script>

<style scoped>
.setting-value {
  font-size: 13px;
  color: var(--text-secondary, #666);
  min-width: 50px;
  text-align: right;
  font-variant-numeric: tabular-nums;
}

.slider {
  width: 140px;
  height: 4px;
  -webkit-appearance: none;
  appearance: none;
  background: var(--border-color, #e8e8e8);
  border-radius: 2px;
  outline: none;
}

.slider::-webkit-slider-thumb {
  -webkit-appearance: none;
  width: 16px;
  height: 16px;
  background: var(--primary-color);
  border-radius: 50%;
  cursor: pointer;
}
</style>