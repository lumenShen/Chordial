<template>
  <div class="chord-fft-controls" :class="{ 'is-immersive': isImmersive }">
    <div class="fft-bars">
      <div
        v-for="(v, i) in bars"
        :key="i"
        class="fft-bar"
        :style="{ height: v + '%' }"
      />
    </div>
  </div>
</template>

<script setup>
/**
 * ChordFftControls — 音频频谱可视化（playerControlsType='fft'）。
 * 用 AnalyserNode 的 fftData 渲染一排频谱条，随音乐跳动。
 */
import { computed, inject } from 'vue'
import { CHORD_FFT_DATA } from '../composables/injectionKeys.js'

const props = defineProps({
  count: { type: Number, default: 48 },
  isImmersive: { type: Boolean, default: false },
})

// 注入 60fps 频谱数据（仅本组件订阅，不影响父组件）
const fftData = inject(CHORD_FFT_DATA, [])

// 把 fftData 采样到 count 根柱子，并映射到 0-100%
const bars = computed(() => {
  const data = fftData.value
  const n = props.count
  if (!data || data.length === 0) return new Array(n).fill(3)
  const step = data.length / n
  const out = new Array(n)
  for (let i = 0; i < n; i++) {
    const idx = Math.min(data.length - 1, Math.floor(i * step))
    const v = Math.max(0.02, data[idx] || 0)
    out[i] = Math.min(100, v * 100)
  }
  return out
})
</script>

<style scoped>
.chord-fft-controls {
  padding: 12px 24px 24px;
  transition: opacity 0.3s, transform 0.3s;
}

.chord-fft-controls.is-immersive {
  opacity: 0;
  pointer-events: none;
  transform: translateY(12px);
}

.fft-bars {
  display: flex;
  align-items: flex-end;
  gap: 3px;
  height: 60px;
  justify-content: center;
}

.fft-bar {
  width: 4px;
  border-radius: 2px;
  background: linear-gradient(180deg, #ffffff, rgba(255, 255, 255, 0.35));
  transition: height 0.08s linear;
}
</style>