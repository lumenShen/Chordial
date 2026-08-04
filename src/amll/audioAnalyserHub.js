/**
 * audioAnalyserHub — 共享 Web Audio 分析器枢纽。
 *
 * # 为什么需要
 * 一个 HTMLMediaElement 只能被 `createMediaElementSource` 连接一次。
 * AMLL 分析器（useAudioAnalyser.js）与自研分析器（useChordAudioAnalyser.js）
 * 都需要从同一个 audio 元素读取频谱。若各自创建 MediaElementSourceNode，
 * 后创建者会抛 `InvalidStateError: already connected`。
 *
 * 本模块在模块级维护唯一的 AudioContext + MediaElementSourceNode，
 * 各分析器通过 createAnalyserTap() 从同一 source "分叉"（fan-out）出自己的
 * AnalyserNode。这是 Web Audio 支持的多路复用方式。
 *
 * # 声音路由
 * 当 source 被创建后，audio 元素输出被重定向进 Web Audio 图。
 * 为保证始终能听到声音且不重复，同一时刻只允许一个"分叉"连到 destination。
 * 因此每个分析器在 start 时 connect 到 destination，stop 时整段 disconnect。
 */
import { PlayerStore } from '@/stores/player/index.js'

// ── 单例状态（模块级，应用生命周期内复用）────────────────────
let audioCtx = null
let sourceNode = null
let boundAudioEl = null

/** 取得或创建 AudioContext 单例（需用户手势触发 resume）。 */
export function ensureAudioContext() {
  if (audioCtx) return audioCtx
  const Ctor = window.AudioContext || window.webkitAudioContext
  if (!Ctor) return null
  audioCtx = new Ctor()
  return audioCtx
}

/**
 * 返回绑定到 audioEl 的共享 MediaElementSourceNode（只创建一次）。
 * 若之前绑定过别的元素，先断开旧 source，再重新绑定。
 */
export function getMediaSourceNode(audioEl) {
  const ctx = ensureAudioContext()
  if (!ctx) return null
  // 已绑定到同一元素 → 复用
  if (boundAudioEl === audioEl && sourceNode) return sourceNode
  // 绑定到其它元素 → 先断开旧 source
  if (sourceNode) {
    try { sourceNode.disconnect() } catch {}
    sourceNode = null
  }
  try {
    sourceNode = ctx.createMediaElementSource(audioEl)
    boundAudioEl = audioEl
  } catch (err) {
    console.warn('[audioAnalyserHub] 绑定 audio 元素失败:', err)
    return null
  }
  return sourceNode
}

/**
 * 从共享 source 分叉出一个 AnalyserNode。
 * @returns {{ analyser: AnalyserNode, connect: () => void, disconnect: () => void } | null}
 */
export function createAnalyserTap() {
  const ctx = ensureAudioContext()
  const source = sourceNode
  if (!ctx || !source) return null
  const analyser = ctx.createAnalyser()
  analyser.fftSize = 256
  analyser.smoothingTimeConstant = 0.8
  source.connect(analyser)
  return {
    analyser,
    /** 连到 destination，使声音可闻（同一时刻仅一个分析器调用）。 */
    connect: () => { try { analyser.connect(ctx.destination) } catch {} },
    /** 整段断开（source→analyser 与 analyser→destination 同时解除）。 */
    disconnect: () => { try { analyser.disconnect() } catch {} },
  }
}

/**
 * 确保当前 audio 元素可供分析（设置 crossOrigin、恢复上下文）。
 * 供各分析器在 start 时调用。
 */
export function ensureAudioReady() {
  const audioEl = PlayerStore.getAudioElement()
  if (!audioEl) return null
  const ctx = ensureAudioContext()
  if (!ctx) return null
  if (ctx.state === 'suspended') ctx.resume().catch(() => {})
  if (!audioEl.crossOrigin) audioEl.crossOrigin = 'anonymous'
  return getMediaSourceNode(audioEl)
}

export default {
  ensureAudioContext,
  getMediaSourceNode,
  createAnalyserTap,
  ensureAudioReady,
}