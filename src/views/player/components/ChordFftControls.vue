<template>
  <div class="chord-fft-controls" :class="{ 'is-immersive': isImmersive }">
    <canvas ref="canvasEl" class="fft-canvas" />
  </div>
</template>

<script setup>
/**
 * ChordFftControls — 音频频谱可视化（playerControlsType='fft'）。
 *
 * # 对齐 AMLL AudioFFTVisualizer
 * - 用 canvas 2D 绘制频谱条（而非 DOM 柱子），大幅降低每帧重排开销。
 * - 频段数据做指数平滑（moving average），视觉过渡自然。
 * - 高频数据经 provide/inject 注入，仅本组件订阅，不影响父组件。
 * - 处理 DPR 与容器 Resize，保证高分屏清晰。
 */
import { ref, inject, onMounted, onBeforeUnmount } from 'vue'
import { CHORD_FFT_DATA } from '../composables/injectionKeys.js'

const props = defineProps({
  count: { type: Number, default: 48 },
  isImmersive: { type: Boolean, default: false },
})

const canvasEl = ref(null)
// 注入 60fps 频谱数据（仅本组件订阅，不影响父组件）
const fftData = inject(CHORD_FFT_DATA, { value: [] })

let ctx = null
let rafId = 0
let running = false
let W = 0
let H = 0
let dpr = 1
// 平滑后的柱子高度（指数移动平均）
let smooth = []

function resize() {
  const canvas = canvasEl.value
  if (!canvas) return
  const rect = canvas.parentElement.getBoundingClientRect()
  dpr = Math.min(window.devicePixelRatio || 1, 2)
  W = Math.max(1, Math.floor(rect.width))
  H = Math.max(1, Math.floor(rect.height))
  canvas.width = Math.floor(W * dpr)
  canvas.height = Math.floor(H * dpr)
  canvas.style.width = W + 'px'
  canvas.style.height = H + 'px'
  ctx = canvas.getContext('2d')
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
}

function draw() {
  if (!ctx) return
  const data = fftData.value
  const n = props.count
  ctx.clearRect(0, 0, W, H)

  if (!data || data.length === 0) {
    // 无数据：绘制极低高度的静音条
    for (let i = 0; i < n; i++) {
      const x = (i / n) * W
      const bw = Math.max(1, W / n - 3)
      ctx.fillStyle = 'rgba(255,255,255,0.35)'
      ctx.fillRect(x, H - 2, bw, 2)
    }
    rafId = requestAnimationFrame(draw)
    return
  }

  const step = data.length / n
  const barW = W / n
  const bw = Math.max(1, barW - 3)
  const gap = 3

  for (let i = 0; i < n; i++) {
    const idx = Math.min(data.length - 1, Math.floor(i * step))
    const v = Math.max(0, data[idx] || 0)
    // 指数平滑：让频谱过渡自然
    const target = Math.min(1, v)
    const prev = smooth[i] ?? 0
    const next = prev + (target - prev) * 0.35
    smooth[i] = next

    const h = Math.max(2, next * H)
    const x = i * barW

    // 高度渐变：顶部更高亮
    const grad = ctx.createLinearGradient(0, H - h, 0, H)
    grad.addColorStop(0, 'rgba(255,255,255,0.95)')
    grad.addColorStop(1, 'rgba(255,255,255,0.25)')
    ctx.fillStyle = grad
    ctx.beginPath()
    ctx.roundRect(x, H - h, bw, h, Math.min(2, bw / 2))
    ctx.fill()
  }

  rafId = requestAnimationFrame(draw)
}

function start() {
  if (running) return
  running = true
  rafId = requestAnimationFrame(draw)
}

function stop() {
  running = false
  if (rafId) { cancelAnimationFrame(rafId); rafId = 0 }
}

let resizeObserver = null

onMounted(() => {
  resize()
  start()
  if (typeof ResizeObserver !== 'undefined') {
    resizeObserver = new ResizeObserver(resize)
    resizeObserver.observe(canvasEl.value?.parentElement)
  }
})

onBeforeUnmount(() => {
  stop()
  resizeObserver?.disconnect()
  resizeObserver = null
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

.fft-canvas {
  display: block;
  width: 100%;
  height: 60px;
}
</style>