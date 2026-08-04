/**
 * injectionKeys — 自研播放器高频数据注入键。
 *
 * # 为什么用 provide/inject 而非 props
 * 播放进度（nowMs）、低频音量（lowFreq）、频谱（fftData）都是 60fps 变化的响应式数据。
 * 若经由根组件 props 下发，会导致 ChordPlayerView 及其整棵子树每帧重渲染，
 * 在低配设备上造成明显卡顿（详见性能审计）。
 *
 * 改为 provide/inject 后，只有真正消费这些数据的叶子组件（进度条、活动歌词行、
 * 流体背景、频谱）订阅并重渲染，其余组件完全不受高频更新影响。
 */
export const CHORD_NOW_MS = Symbol('chordNowMs')
export const CHORD_LOW_FREQ = Symbol('chordLowFreq')
export const CHORD_FFT_DATA = Symbol('chordFftData')