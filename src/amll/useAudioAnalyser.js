/**
 * useAudioAnalyser — 将 HTMLAudioElement 接入 Web Audio API，
 * 实时计算低频音量（lowFreqVolume）和频域数据（fftData），
 * 推送给 AMLL Jotai store，驱动流体背景随音乐跳动。
 *
 * # 设计要点
 * - AnalyserNode 单例：audio 元素切换 src 时不需重建 AudioContext，
 *   只在首次调用时惰性创建。
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
import { PlayerStore } from '@/stores/player/index.js'

// ── 常量 ──────────────────────────────────────────────────────
const FFT_SIZE = 256              // AnalyserNode.fftSize（必须是 2 的幂）
const BIN_COUNT = FFT_SIZE / 2    // 频段数 = 128，但只用前 32 个（低频段）
const SMOOTHING = 0.8             // AnalyserNode.smoothingTimeConstant

// ── 单例状态（模块级，多次调用 useAudioAnalyser 复用同一实例）──
let audioCtx = null
let analyser = null
let sourceNode = null
let freqDataBuffer = null         // Uint8Array(FFT_SIZE / 2)
let rafId = 0
let running = false

// 当前 audio 元素引用，用于判断是否需要重新绑定 sourceNode
let boundAudioEl = null

// lowFreqVolume 计算频段（Hz），可由 setLowFreqRange 动态更新
// 默认 80-120Hz（AMLL 文档推荐，对应低音鼓点）
let lowFreqRange = [80, 120]

// ── 缓存与复用：避免 60fps 每帧分配 ──────────────────────────────
//
// `fftDataAtom` 期望 number[]，Jotai 用 Object.is 比较引用判断是否触发订阅者。
// 因此不能复用同一个数组（引用不变 → Jotai 不通知）。
// 采用双缓冲 ping-pong：仅分配 2 个数组，每帧交替返回，引用会变化但无 GC 压力。
const fftBufferA = new Array(BIN_COUNT)
const fftBufferB = new Array(BIN_COUNT)
let fftBufferFlip = false

// lowFreqVolume 的频段索引缓存：sampleRate / freqData.length / lowFreqRange
// 在一次播放会话内都不变，每帧重算 Math.floor/ceil 是纯浪费。
let cachedLowBin = -1
let cachedHighBin = -1
let lowFreqCacheKey = ''

/**
 * 取得或创建 AudioContext + AnalyserNode 单例。
 * 之所以惰性创建：浏览器要求 AudioContext 必须由用户手势触发 resume，
 * 在页面加载时就建会导致 context state = 'suspended'。
 */
function ensureContext() {
	if (audioCtx) return audioCtx
	// 兼容 webkitAudioContext（旧 iOS）
	const Ctor = window.AudioContext || window.webkitAudioContext
	if (!Ctor) {
		console.warn('[useAudioAnalyser] Web Audio API 不可用')
		return null
	}
	audioCtx = new Ctor()
	analyser = audioCtx.createAnalyser()
	analyser.fftSize = FFT_SIZE
	analyser.smoothingTimeConstant = SMOOTHING
	freqDataBuffer = new Uint8Array(analyser.frequencyBinCount)
	return audioCtx
}

/**
 * 把 audio 元素接到 AnalyserNode 上。
 * MediaElementSourceNode 一旦创建就被"消耗"，同一元素不能 createMediaElementSource 两次，
 * 所以缓存 boundAudioEl 避免重复绑定。
 */
function bindAudioElement(audioEl) {
	if (!audioCtx || !analyser) return
	if (boundAudioEl === audioEl) return

	// 如果之前绑过别的元素，先断开旧 sourceNode
	if (sourceNode) {
		try { sourceNode.disconnect() } catch {}
	}

	try {
		sourceNode = audioCtx.createMediaElementSource(audioEl)
		sourceNode.connect(analyser)
		analyser.connect(audioCtx.destination)
		boundAudioEl = audioEl
	} catch (err) {
		// 常见错误：同一元素被重复 createMediaElementSource
		console.warn('[useAudioAnalyser] 绑定 audio 元素失败:', err)
	}
}

/**
 * 从频域数据计算 lowFreqVolume（0-1）。
 * 频段范围由 lowFreqRange 决定（默认 80-120Hz，可由 setLowFreqRange 动态更新）。
 * AMLL 用这个值驱动背景鼓点跳动。
 */
function computeLowFreqVolume(freqData) {
	if (!audioCtx || !analyser) return 0
	const len = freqData.length
	const [lowHz, highHz] = lowFreqRange
	// 频段索引在一次会话内不变，缓存避免每帧 Math.floor/ceil
	const key = `${audioCtx.sampleRate}:${len}:${lowHz}:${highHz}`
	if (key !== lowFreqCacheKey) {
		const nyquist = audioCtx.sampleRate / 2
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
	// Uint8Array 取值 0-255，归一化到 0-1
	return sum / count / 255
}

/**
 * 抽取 fftData：取前 BIN_COUNT 个频段，归一化到 0-1。
 * AMLL 的 fftDataAtom 期望 number[]，长度无严格要求（一般 32-64）。
 *
 * 使用双缓冲 ping-pong：Jotai 用 Object.is 比较引用判断是否通知订阅者，
 * 复用同一数组会导致不触发更新；每帧 new Array 又造成 60fps GC 压力。
 * 双缓冲仅分配 2 次，每帧交替返回，引用变化且零分配。
 */
function computeFftData(freqData) {
	// 取前 32 个频段（低中频，对背景可视化最有信息量）
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
		if (!running || !analyser || !freqDataBuffer) {
			running = false
			return
		}
		analyser.getByteFrequencyData(freqDataBuffer)
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
	const audioEl = PlayerStore.getAudioElement()
	if (!audioEl) {
		console.warn('[useAudioAnalyser] PlayerStore.audioElement 尚未就绪')
		return
	}
	const ctx = ensureContext()
	if (!ctx) return

	// AudioContext 可能因自动播放策略处于 suspended 状态
	if (ctx.state === 'suspended') {
		ctx.resume().catch(() => {})
	}

	// audioEl crossOrigin 必须设为 anonymous 才能读取频谱数据
	// （Tauri 协议层已返回 CORS 头）
	if (!audioEl.crossOrigin) audioEl.crossOrigin = 'anonymous'

	bindAudioElement(audioEl)
	startLoop(store, atoms)
}

/** 停止音频分析（PlayerView 卸载时调用） */
export function stopAudioAnalyser() {
	stopLoop()
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
}

export default { startAudioAnalyser, stopAudioAnalyser, setLowFreqRange }
