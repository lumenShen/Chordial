<template>
  <div class="settings-section">
    <h3 class="block-title">
      <HIcon name="droplet-half" />
      流体背景
    </h3>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">垂直同步（VSync）</label>
        <span class="setting-desc">跟随显示器刷新率渲染，开启后下方帧率选项失效</span>
      </div>
      <div class="setting-control">
        <label class="toggle">
          <input type="checkbox" v-model="settings.lyricBackgroundVSync" />
          <span class="toggle-slider"></span>
        </label>
      </div>
    </div>

    <div class="setting-item" :class="{ 'is-disabled': settings.lyricBackgroundVSync }">
      <div class="setting-info">
        <label class="setting-label">背景帧率</label>
        <span class="setting-desc">较低可提升性能，性能影响高；垂直同步开启时此选项失效</span>
      </div>
      <div class="setting-control">
        <select
          v-model.number="settings.lyricBackgroundFPS"
          class="select"
          :disabled="settings.lyricBackgroundVSync"
        >
          <option :value="30">30 FPS</option>
          <option :value="45">45 FPS</option>
          <option :value="60">60 FPS</option>
          <option :value="90">90 FPS</option>
          <option :value="120">120 FPS</option>
        </select>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">鼓点频段下限</label>
        <span class="setting-desc">驱动背景跳动的频段下限（Hz）。默认 4000；可在输入框输入超出滑块范围的值</span>
      </div>
      <div class="setting-control">
        <input
          type="range" min="20" max="500" step="5"
          v-model.number="lowFreqLow"
          class="slider"
        />
        <input
          type="number" min="0" max="20000" step="5"
          v-model.number="lowFreqLow"
          class="freq-input"
        />
        <span class="setting-value">Hz</span>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">鼓点频段上限</label>
        <span class="setting-desc">驱动背景跳动的频段上限（Hz）。默认 10000；可在输入框输入超出滑块范围的值</span>
      </div>
      <div class="setting-control">
        <input
          type="range" :min="lowFreqLow + 5" max="2000" step="5"
          v-model.number="lowFreqHigh"
          class="slider"
        />
        <input
          type="number" :min="lowFreqLow + 5" max="20000" step="5"
          v-model.number="lowFreqHigh"
          class="freq-input"
        />
        <span class="setting-value">Hz</span>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">渲染缩放</label>
        <span class="setting-desc">0.5 降低一半分辨率以提升性能，1 原始分辨率</span>
      </div>
      <div class="setting-control">
        <input
          type="range" min="0.25" max="1" step="0.05"
          v-model.number="settings.lyricBackgroundRenderScale"
          class="slider"
        />
        <span class="setting-value">{{ settings.lyricBackgroundRenderScale.toFixed(2) }}</span>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">静态背景模式</label>
        <span class="setting-desc">封面切换后保持静止，禁用鼓点跳动以节省性能</span>
      </div>
      <div class="setting-control">
        <label class="toggle">
          <input type="checkbox" v-model="settings.lyricBackgroundStaticMode" />
          <span class="toggle-slider"></span>
        </label>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">纯色/CSS 背景</label>
        <span class="setting-desc">当背景渲染器为纯色时使用此颜色</span>
      </div>
      <div class="setting-control">
        <input
          type="color"
          v-model="settings.cssBackgroundProperty"
          class="color-input"
        />
        <span class="setting-value">{{ settings.cssBackgroundProperty }}</span>
      </div>
    </div>
  </div>
</template>

<script setup>
import { useLyricsSettings } from '@/composables/useLyricsSettings.js';

const { settings, lowFreqLow, lowFreqHigh } = useLyricsSettings();
</script>

<style scoped>
.setting-item.is-disabled {
  opacity: 0.45;
  pointer-events: none;
}

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

/* 鼓点频段输入框：与滑块并排，允许输入超出滑块范围的值 */
.freq-input {
  width: 72px;
  padding: 5px 8px;
  font-size: 13px;
  border: 1px solid var(--border-color, #e8e8e8);
  border-radius: 6px;
  background: var(--bg-primary, #fff);
  color: var(--text-primary, #333);
  outline: none;
  font-family: inherit;
  font-variant-numeric: tabular-nums;
  text-align: right;
  /* 隐藏原生上下箭头，保持视觉简洁 */
  -moz-appearance: textfield;
}

.freq-input::-webkit-outer-spin-button,
.freq-input::-webkit-inner-spin-button {
  -webkit-appearance: none;
  margin: 0;
}

.freq-input:focus {
  border-color: var(--primary-color);
}

.color-input {
  width: 36px;
  height: 28px;
  padding: 0;
  border: 1px solid var(--border-color, #e8e8e8);
  border-radius: 6px;
  background: transparent;
  cursor: pointer;
}

.color-input::-webkit-color-swatch-wrapper {
  padding: 2px;
}

.color-input::-webkit-color-swatch {
  border: none;
  border-radius: 4px;
}

@media (max-width: 767px) {
  .select {
    min-width: 0;
    flex: 1;
  }
}
</style>