<template>
  <div class="chord-fluid-bg" aria-hidden="true">
    <canvas ref="canvasEl" />
  </div>
</template>

<script setup>
/**
 * ChordFluidBackground — 自研播放器流体背景。
 *
 * 用 canvas 2D 绘制受音频驱动的动态渐变流体（对齐 AMLL 的 MeshGradient 观感）。
 *
 * # 性能策略
 * - 低分辨率离屏渲染（约 1/4 分辨率）+ CSS 放大，大幅减少像素填充。
 * - 渐变点数量固定（6 个），radius 随 lowFreqVolume 脉冲，零分配。
 * - 每帧仅重绘 canvas，不触发布局；CSS 放大不参与重采样。
 *
 * # 颜色来源
 * - 优先从封面 URL 提取主色（Image 加载后取平均色），作为初始渐变基调。
 * - 兜底用深色渐变（--amll-bg 默认 #111）。
 */
import { ref, inject, onMounted, onBeforeUnmount, watch } from 'vue'
import { CHORD_LOW_FREQ } from '../composables/injectionKeys.js'

const props = defineProps({
  coverUrl: { type: String, default: '' },
  // 静态模式：不随音频跳动，仅缓慢流动
  staticMode: { type: Boolean, default: false },
})

// 注入 60fps 低频音量 ref。仅在本组件的 rAF draw 循环内读取 .value，
// 不参与模板渲染，因此不会触发 vue 组件重渲染。
const lowFreqRef = inject(CHORD_LOW_FREQ, ref(0))

let ctx = null
let canvas = null
let rafId = 0
let running = false
let W = 0
let H = 0
let dpr = 1

// 渐变点（固定 6 个，位置随时间缓慢漂移）
const blobs = Array.from({ length: 6 }, (_, i) => ({
  x: Math.random(),
  y: Math.random(),
  r: 0.4 + Math.random() * 0.3,
  dx: (Math.random() - 0.5) * 0.0004,
  dy: (Math.random() - 0.5) * 0.0004,
  hue: i * 55,
}))

// 封面主色（提取后作为背景基调）
const baseColors = ref([
  { r: 36, g: 36, b: 40 },
  { r: 20, g: 24, b: 40 },
  { r: 40, g: 28, b: 44 },
])

function resize() {
  if (!canvas) return
  const rect = canvas.parentElement.getBoundingClientRect()
  dpr = Math.min(window.devicePixelRatio || 1, 1.5)
  // 低分辨率渲染：目标 1/4 宽，限制最大尺寸
  const scale = Math.max(0.25, Math.min(0.5, 1 / dpr))
  W = Math.max(64, Math.floor(rect.width * scale))
  H = Math.max(64, Math.floor(rect.height * scale))
  canvas.width = W
  canvas.height = H
  ctx = canvas.getContext('2d')
}

function extractCoverColors() {
  if (!props.coverUrl) return
  const img = new Image()
  img.crossOrigin = 'anonymous'
  img.onload = () => {
    try {
      const c = document.createElement('canvas')
      c.width = 48
      c.height = 48
      const g = c.getContext('2d')
      g.drawImage(img, 0, 0, 48, 48)
      const data = g.getImageData(0, 0, 48, 48).data
      // 采样 4 个角 + 中心，计算平均主色
      const samples = [
        [6, 6], [42, 6], [6, 42], [42, 42], [24, 24],
      ]
      const colors = samples.map(([x, y]) => {
        const i = (y * 48 + x) * 4
        return { r: data[i], g: data[i + 1], b: data[i + 2] }
      })
      // 平均
      const avg = colors.reduce(
        (acc, c) => ({ r: acc.r + c.r, g: acc.g + c.g, b: acc.b + c.b }),
        { r: 0, g: 0, b: 0 },
      )
      const n = colors.length
      const main = { r: avg.r / n, g: avg.g / n, b: avg.b / n }
      // 深色化 + 微调，生成三个基调
      baseColors.value = [
        { r: main.r * 0.28, g: main.g * 0.28, b: main.b * 0.32 },
        { r: main.r * 0.18, g: main.g * 0.22, b: main.b * 0.28 },
        { r: main.r * 0.34, g: main.g * 0.2, b: main.b * 0.24 },
      ]
    } catch {
      /* 忽略：保持默认颜色 */
    }
  }
  img.onerror = () => {}
  img.src = props.coverUrl
}

function draw() {
  if (!ctx) return
  const t = performance.now() / 1000
  const lowFreq = lowFreqRef.value
  const pulse = props.staticMode ? 0 : lowFreq * 0.25

  // 背景底色
  const [c0, c1, c2] = baseColors.value
  const bg = ctx.createLinearGradient(0, 0, W, H)
  bg.addColorStop(0, `rgb(${c0.r},${c0.g},${c0.b})`)
  bg.addColorStop(1, `rgb(${c2.r},${c2.g},${c2.b})`)
  ctx.fillStyle = bg
  ctx.fillRect(0, 0, W, H)

  // 流体渐变点
  for (let i = 0; i < blobs.length; i++) {
    const b = blobs[i]
    // 缓慢漂移
    b.x += b.dx
    b.y += b.dy
    if (b.x < -0.2) b.x = 1.2
    if (b.x > 1.2) b.x = -0.2
    if (b.y < -0.2) b.y = 1.2
    if (b.y > 1.2) b.y = -0.2

    const wobble = Math.sin(t * 0.3 + i * 1.7) * 0.05
    const radius = Math.max(W, H) * (b.r + wobble + pulse)
    const color = c1

    const grad = ctx.createRadialGradient(
      b.x * W, b.y * H, 0,
      b.x * W, b.y * H, radius,
    )
    const alpha = 0.12 + pulse * 0.5
    grad.addColorStop(0, `rgba(${color.r},${color.g},${color.b},${alpha})`)
    grad.addColorStop(1, `rgba(${color.r},${color.g},${color.b},0)`)
    ctx.fillStyle = grad
    ctx.fillRect(0, 0, W, H)
  }

  // 低频脉冲光晕
  if (!props.staticMode) {
    const glowR = Math.max(W, H) * (0.3 + lowFreq * 0.5)
    const g = ctx.createRadialGradient(W / 2, H / 2, 0, W / 2, H / 2, glowR)
    g.addColorStop(0, `rgba(255,255,255,${lowFreq * 0.06})`)
    g.addColorStop(1, 'rgba(255,255,255,0)')
    ctx.fillStyle = g
    ctx.fillRect(0, 0, W, H)
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

watch(() => props.coverUrl, () => extractCoverColors())

onMounted(() => {
  resize()
  extractCoverColors()
  start()
  window.addEventListener('resize', resize)
})

onBeforeUnmount(() => {
  stop()
  window.removeEventListener('resize', resize)
})
</script>

<style scoped>
.chord-fluid-bg {
  position: absolute;
  inset: 0;
  overflow: hidden;
  background: #111111;
  z-index: 0;
}

.chord-fluid-bg canvas {
  width: 100%;
  height: 100%;
  display: block;
  /* 低分辨率放大 + 模糊，制造柔和流体感 */
  filter: blur(18px) saturate(1.3);
  transform: scale(1.08);
  will-change: transform;
}
</style>