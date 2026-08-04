/**
 * useChordAudioAnalyser — 自研播放器专用音频分析器（响应式 ref 版本）。
 *
 * 复用 /src/amll/audioAnalyserHub.js 的共享 AudioContext + MediaElementSourceNode，
 * 输出改为响应式 ref，供自研 Vue 播放器的流体背景直接消费。
 *
 * # 设计要点
 * - 分析器节点从共享 hub 分叉（fan-out），避免「同一元素二次 createMediaElementSource」崩溃。
 * - 仅在 PlayerView 挂载时启动，卸载后立即停止，零后台开销。
 * - fftSize = 256：32 个频段，足够驱动背景动画，CPU 开销极低。
 * - lowFreqVolume 取 80-120Hz 频段均值，归一化到 0-1（驱动鼓点跳动）。
 * - fftData 使用双缓冲 ping-pong，避免每帧分配数组造成 GC 压力。
 */
import { ref, provide, onScopeDispose } from 'vue'
import { ensureAudioReady, createAnalyserTap } from '@/amll/audioAnalyserHub.js'
import { CHORD_LOW_FREQ, CHORD_FFT_DATA } from './injectionKeys.js'

// ── 常量 ──────────────────────────────────────────────────────
const BIN_COUNT = 128 // AnalyserNode.frequencyBinCount（fftSize/2）
const SMOOTHING = 0.8

// ── 分析器节点（本书作用域内保持）────────────────────────────
let analyserNode = null
let tap = null

// ── 双缓冲 fftData（避免 60fps 每帧分配）──────────────────────
const fftBufferA = new Array(BIN_COUNT)
const fftBufferB = new Array(BIN_COUNT)
let fftBufferFlip = false

// 低频频段计算缓存
let lowFreqRange = [80, 120]
let cachedLowBin = -1
let cachedHighBin = -1
let lowFreqCacheKey = ''

function computeLowFreqVolume(freqData) {
  if (!analyserNode) return 0
  const ctx = analyserNode.context
  const len = freqData.length
  const [lowHz, highHz] = lowFreqRange
  const key = `${ctx.sampleRate}:${len}:${lowHz}:${highHz}`
  if (key !== lowFreqCacheKey) {
    const nyquist = ctx.sampleRate / 2
    const binHz = nyquist / len
    cachedLowBin = Math.max(1, Math.floor(lowHz / binHz))
    cachedHighBin = Math.min(len - 1, Math.ceil(highHz / binHz))
    lowFreqCacheKey = key
  }
  if (cachedHighBin < cachedLowBin) return 0
  let sum = 0
  let count = 0
  for (let i = cachedLowBin; i <= cachedHighBin; i++) {
    sum += freqData[i]
    count++
  }
  if (count === 0) return 0
  return sum / count / 255
}

function computeFftData(freqData) {
  const out = fftBufferFlip ? fftBufferA : fftBufferB
  fftBufferFlip = !fftBufferFlip
  for (let i = 0; i < BIN_COUNT; i++) {
    out[i] = freqData[i] / 255
  }
  return out
}

/**
 * 启动音频分析，返回响应式 ref。
 * @returns {{ lowFreqVolume: import('vue').Ref<number>, fftData: import('vue').Ref<number[]> }}
 */
export function useChordAudioAnalyser() {
  const lowFreqVolume = ref(0)
  const fftData = ref([])
  let rafId = 0
  let running = false

  function start() {
    if (running) return
    // 确保共享 source 就绪
    if (!ensureAudioReady()) return
    // 防御：若上次 tap 未断开则先断开，避免 source→analyser 连接累积
    tap?.disconnect()
    // 从共享 source 分叉出本分析器节点
    tap = createAnalyserTap()
    if (!tap) return
    analyserNode = tap.analyser
    // 连接 destination 使声音可闻（同一时刻仅一个分析器连接）
    tap.connect()

    running = true
    const freqDataBuffer = new Uint8Array(analyserNode.frequencyBinCount)
    const tick = () => {
      if (!running || !analyserNode) { running = false; return }
      analyserNode.getByteFrequencyData(freqDataBuffer)
      lowFreqVolume.value = computeLowFreqVolume(freqDataBuffer)
      fftData.value = computeFftData(freqDataBuffer)
      rafId = requestAnimationFrame(tick)
    }
    rafId = requestAnimationFrame(tick)
  }

  function stop() {
    running = false
    if (rafId) { cancelAnimationFrame(rafId); rafId = 0 }
    // 整段断开，把 destination 路由让给下一个分析器
    tap?.disconnect()
    tap = null
    analyserNode = null
  }

  // 挂载即启动
  start()

  // 高频音频数据通过 provide 下发，仅流体背景/频谱订阅，
  // 避免经 props 下发导致整棵播放器树每帧重渲染。
  provide(CHORD_LOW_FREQ, lowFreqVolume)
  provide(CHORD_FFT_DATA, fftData)

  onScopeDispose(stop)

  return { lowFreqVolume, fftData }
}

export default useChordAudioAnalyser