<template>
  <div
    ref="barEl"
    class="chord-progress-bar"
    :class="{ dragging: isDragging }"
    @pointerdown="onPointerDown"
  >
    <div class="bar-track">
      <div class="bar-fill" :style="{ width: fillPct + '%' }" />
      <div class="bar-buffer" :style="{ width: bufferPct + '%' }" />
      <div class="bar-thumb" :style="{ left: fillPct + '%' }" />
    </div>
  </div>
</template>

<script setup>
/**
 * ChordProgressBar — 可拖拽进度条。
 * 指针按下/拖动时本地实时预览，松手才 commit seek（对齐 AMLL BouncingSlider 的
 * changeOnDrag=false 行为），显示填充与缓冲。
 *
 * # 性能：进度经 inject 高频驱动
 * 播放进度 by 帧更新，为避免父组件链每帧重渲染，进度 ref 由
 * ChordPlayerView 通过 CHORD_NOW_MS provide 下发，本组件直接注入。
 */
import { ref, computed, inject, onBeforeUnmount } from 'vue'
import { CHORD_NOW_MS } from '../composables/injectionKeys.js'

const props = defineProps({
  durationMs: { type: Number, default: 0 },
  bufferMs: { type: Number, default: 0 },
})

const emit = defineEmits(['seek'])

const barEl = ref(null)
const isDragging = ref(false)
const dragPct = ref(-1)
// 注入 60fps 播放进度（仅本组件订阅，不影响父组件）
const nowMs = inject(CHORD_NOW_MS, ref(0))

const fillPct = computed(() => {
  if (isDragging.value && dragPct.value >= 0) return dragPct.value
  if (!props.durationMs) return 0
  return Math.min(100, (nowMs.value / props.durationMs) * 100)
})

const bufferPct = computed(() => {
  if (!props.durationMs) return 0
  return Math.min(100, (props.bufferMs / props.durationMs) * 100)
})

function pctFromEvent(e) {
  const rect = barEl.value.getBoundingClientRect()
  const pct = ((e.clientX - rect.left) / rect.width) * 100
  return Math.min(100, Math.max(0, pct))
}

function onPointerDown(e) {
  e.preventDefault()
  isDragging.value = true
  dragPct.value = pctFromEvent(e)
  window.addEventListener('pointermove', onPointerMove)
  window.addEventListener('pointerup', onPointerUp)
}

function onPointerMove(e) {
  if (!isDragging.value) return
  dragPct.value = pctFromEvent(e)
}

function onPointerUp(e) {
  if (!isDragging.value) return
  const pct = pctFromEvent(e)
  isDragging.value = false
  dragPct.value = -1
  window.removeEventListener('pointermove', onPointerMove)
  window.removeEventListener('pointerup', onPointerUp)
  if (props.durationMs) {
    emit('seek', (pct / 100) * props.durationMs)
  }
}

onBeforeUnmount(() => {
  window.removeEventListener('pointermove', onPointerMove)
  window.removeEventListener('pointerup', onPointerUp)
})
</script>

<style scoped>
.chord-progress-bar {
  width: 100%;
  padding: 8px 0;
  cursor: pointer;
  touch-action: none;
}

.bar-track {
  position: relative;
  height: 4px;
  border-radius: 2px;
  background: rgba(255, 255, 255, 0.18);
  overflow: visible;
  /* 对齐 AMLL BouncingSlider：hover/拖动时加粗（expand），结束收缩（collapse）。
     仅作用于轨道高度，不触碰 60fps 热更新的 fill/buffer 宽度，性能安全。 */
  transition: height 0.18s ease;
}

.bar-buffer {
  position: absolute;
  top: 0;
  left: 0;
  height: 100%;
  border-radius: 2px;
  background: rgba(255, 255, 255, 0.12);
}

.bar-fill {
  position: absolute;
  top: 0;
  left: 0;
  height: 100%;
  border-radius: 2px;
  background: #ffffff;
}

.bar-thumb {
  position: absolute;
  top: 50%;
  width: 12px;
  height: 12px;
  border-radius: 50%;
  background: #fff;
  transform: translate(-50%, -50%);
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.4);
  opacity: 0;
  transition: opacity 0.2s, width 0.18s ease, height 0.18s ease;
}

/* hover/拖动时轨道加粗、缩略图放大（对齐 AMLL expand 交互） */
.chord-progress-bar:hover .bar-track,
.chord-progress-bar.dragging .bar-track {
  height: 6px;
}

.chord-progress-bar:hover .bar-thumb,
.chord-progress-bar.dragging .bar-thumb {
  opacity: 1;
  width: 14px;
  height: 14px;
}
</style>