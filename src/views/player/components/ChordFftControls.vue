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
 * - 条数由宽度决定（8px 间距），而非固定数量，与 AMLL 一致。
 * - 白色圆头描线（stroke line），无间隙、无渐变填充，与 AMLL 一致。
 * - 自动缩放：滑动最大值（maxValue），让频谱随音量/电平自适应，与 AMLL 一致。
 * - 高度用 norm^2 二次映射，低频频段做衰减，视觉过渡自然。
 * - 平滑用 AMLL 的 buf=(buf+2*t)/3 IIR，缓冲数组复用、零每帧分配。
 * - 高频数据经 provide/inject 注入，仅本组件订阅，不影响父组件。
 * - 处理 DPR 与容器 Resize，保证高分屏清晰。
 */
import { ref, inject, onMounted, onBeforeUnmount } from 'vue'
import { CHORD_FFT_DATA } from '../composables/injectionKeys.js'

const props = defineProps({
  // 保留兼容；实际条数由宽度决定（对齐 AMLL），此处不参与绘制
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
// 平滑后的频段缓冲（复用，避免每帧分配）
let smooth = new Array(128).fill(0)
// 重采样后的显示缓冲（复用）
let display = new Array(128).fill(0)
// 自动缩放滑动最大值（对齐 AMLL maxValue 语义）
let maxValue = 0.5

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

/** 线性重采样到目标长度，写回 dst（复用，不分配） */
function resample(src, dst, dstLen) {
  const n = src.length
  if (n <= 0 || dstLen <= 0) return
  if (n === dstLen) {
    for (let i = 0; i < n; i++) dst[i] = src[i]
    return
  }
  const scale = (n - 1) / Math.max(1, dstLen - 1)
  for (let i = 0; i < dstLen; i++) {
    const x = i * scale
    const x0 = Math.floor(x)
    const x1 = Math.min(n - 1, x0 + 1)
    const t = x - x0
    dst[i] = src[x0] * (1 - t) + src[x1] * t
  }
}

function draw() {
  if (!ctx) return
  const data = fftData.value
  ctx.clearRect(0, 0, W, H)

  if (!data || data.length === 0) {
    // 对齐 AMLL：无数据/暂停时画布静默（仅清空，不绘制静音条）
    rafId = requestAnimationFrame(draw)
    return
  }

  const n = data.length
  if (smooth.length !== n) smooth.length = n

  // 平滑 + 低频频段衰减（对齐 AMLL：t = raw * min((i+5)/len*4, 1)，buf=(buf+2*t)/3）
  for (let i = 0; i < n; i++) {
    const raw = data[i] ?? 0
    const atten = Math.min((i + 5) / n * 4, 1)
    const t = raw * atten
    const prev = smooth[i] ?? 0
    smooth[i] = (prev + t * 2) / 3
  }

  // 条数：8px 间距（对齐 AMLL desiredSpacing=8*dpr，此处为 CSS 像素），至少 8 条
  let barCount = Math.floor(W / 8)
  barCount = Math.max(8, Math.min(barCount, n))
  resample(smooth, display, barCount)

  // 自动缩放：滑动最大值（对齐 AMLL maxValue = max(maxValue*0.9 + target*0.1, 下限)）
  let localMax = 0
  for (let i = 0; i < barCount; i++) {
    if (display[i] > localMax) localMax = display[i]
  }
  maxValue = Math.max(maxValue * 0.9 + localMax * 0.1, 0.05)

  const barWidth = W / barCount
  const beginY = H - barWidth
  const range = H - barWidth * 2

  // 白色圆头描线（对齐 AMLL：strokeStyle white、lineCap/lineJoin round、lineWidth 4）
  ctx.strokeStyle = 'rgba(255,255,255,0.92)'
  ctx.lineWidth = 4
  ctx.lineCap = 'round'
  ctx.lineJoin = 'round'
  ctx.beginPath()
  for (let i = 0; i < barCount; i++) {
    const norm = Math.min(1, Math.max(0, display[i] / maxValue))
    const x = (i + 0.5) * barWidth
    ctx.moveTo(x, beginY)
    ctx.lineTo(x, beginY - norm * norm * range)
  }
  ctx.stroke()

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