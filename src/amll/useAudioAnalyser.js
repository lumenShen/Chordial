/**
 * useAudioAnalyser — 将 HTMLAudioElement 接入 Web Audio API，
 * 实时计算低频音量（lowFreqVolume）和频域数据（fftData），
 * 推送给 AMLL Jotai store，驱动流体背景随音乐跳动。
 *
 * # 设计要点
 * - 共享 audioAnalyserHub 提供的 AudioContext + MediaElementSourceNode，
 *   与自研播放器分析器（useChordAudioAnalyser）fan-out 复用同一 source，
 *   避免「同一元素二次 createMediaElementSource」崩溃。
 * - requestAnimationFrame 节流：浏览器原生对齐渲染帧，避免空转。
 * - 仅在 PlayerView 打开时启动分析，关闭后立即停止，零后台开销。
 * - fftSize = 256：32 个频段，足够驱动背景动画，CPU 开销极低。
 * - lowFreqVolume 取 80-120Hz 频段均值，归一化到 0-1。
 *
 * # CORS 注意
 * chordial:// 协议返回的音频流必须带 CORS 头，否则 crossOrigin="anonymous"
 * 的 audio 元素会被 Tauri WebView 拒绝读取频谱数据（ImageData tainted）。
 * 媒体协议层已在 to_tauri_response 注入 Access-Control-Allow-Origin: *。
 */
import { ensureAudioReady, createAnalyserTap } from '@/amll/audioAnalyserHub.js'

// ── 常量 ──────────────────────────────────────────────────────
const BIN_COUNT = 128 // AnalyserNode.frequencyBinCount（fftSize/2）

// ── 分析器节点（startAudioAnalyser 期间持有）────────────────────
let analyserNode = null
let tap = null

// 当前音频分析器节点（用于 getByteFrequencyData 时的上下文）
let freqDataBuffer = null

// ── 缓存与复用：避免 60fps 每帧分配 ──────────────────────────────
//
// `fftDataAtom` 期望 number[]，Jotai 用 Object.is 比较引用判断是否触发订阅者。
// 因此不能复用同一个数组（引用不变 → Jotai 不通知）。
// 采用双缓冲 ping-pong：仅分配 2 个数组，每帧交替返回，引用会变化但无 GC 压力。
const fftBufferA = new Array(BIN_COUNT)
const fftBufferB = new Array(BIN_COUNT)
let fftBufferFlip = false

// lowFreqVolume 的频段索引缓存
let lowFreqRange = [80, 120]
let cachedLowBin = -1
let cachedHighBin = -1
let lowFreqCacheKey = ''

let rafId = 0
let running = false

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
 * 启动音频分析循环。
 * @param {import('jotai').Store} store - AMLL Jotai store
 * @param {object} atoms - 需要写入的 atoms 引用
 * @param {import('@applemusic-like-lyrics/react-full').PrimitiveAtom<number>} atoms.lowFreqVolumeAtom
 * @param {import('@applemusic-like-lyrics/react-full').PrimitiveAtom<number[]>} atoms.fftDataAtom
 */
function startLoop(store, atoms) {
  if (running) return
  running = true

  const tick = () => {
    if (!running || !analyserNode || !freqDataBuffer) {
      running = false
      return
    }
    analyserNode.getByteFrequencyData(freqDataBuffer)
    store.set(atoms.lowFreqVolumeAtom, computeLowFreqVolume(freqDataBuffer))
    store.set(atoms.fftDataAtom, computeFftData(freqDataBuffer))
    rafId = requestAnimationFrame(tick)
  }
  rafId = requestAnimationFrame(tick)
}

/** 停止分析循环 */
function stopLoop() {
  running = false
  if (rafId) {
    cancelAnimationFrame(rafId)
    rafId = 0
  }
}

/**
 * 启动音频分析（PlayerView 挂载时调用）。
 * 会自动接管 PlayerStore 的 audioElement。
 */
export function startAudioAnalyser(store, atoms) {
  // 确保共享 source 就绪（含 crossOrigin、AudioContext resume）
  if (!ensureAudioReady()) return

  // 从共享 source 分叉出本分析器节点
  tap = createAnalyserTap()
  if (!tap) return
  analyserNode = tap.analyser
  // 连接 destination 使声音可闻（同一时刻仅一个分析器连接）
  tap.connect()

  freqDataBuffer = new Uint8Array(analyserNode.frequencyBinCount)
  startLoop(store, atoms)
}

/** 停止音频分析（PlayerView 卸载时调用） */
export function stopAudioAnalyser() {
  stopLoop()
  // 整段断开，把 destination 路由让给下一个分析器
  tap?.disconnect()
  tap = null
  analyserNode = null
  freqDataBuffer = null
}

/**
 * 更新 lowFreqVolume 计算频段（Hz）。
 * 由桥接层 watch AmllSettingsStore.lowFreqVolumeRange 后调用，
 * 实现设置页改频段时实时生效。
 *
 * @param {[number, number]} range - [低频Hz, 高频Hz]
 */
export function setLowFreqRange(range) {
  if (!Array.isArray(range) || range.length !== 2) return
  const [low, high] = range
  if (typeof low !== 'number' || typeof high !== 'number' || low >= high || low < 0) return
  lowFreqRange = [low, high]
  // 频段缓存失效
  lowFreqCacheKey = ''
}

export default { startAudioAnalyser, stopAudioAnalyser, setLowFreqRange }