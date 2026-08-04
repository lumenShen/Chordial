<template>
  <div ref="scrollEl" class="chord-lyrics-view" @click="onBackgroundClick">
    <div class="lyrics-spacer-top" />
    <div
      v-for="(line, i) in lyricLines"
      :key="i"
      :ref="(el) => setLineRef(el, i)"
      class="lyrics-line-wrap"
    >
      <ChordLyricLine
        :line="line"
        :is-active="i === activeLineIndex"
        :is-before="i < activeLineIndex"
        :is-after="i > activeLineIndex"
        :show-translation="showTranslation"
        :font-size-px="fontSizePx"
        :font-weight="fontWeight"
        :letter-spacing="letterSpacing"
        :enable-blur="enableBlur"
        :enable-scale="enableScale"
        :enable-spring="enableSpring"
        @click="onLineClick"
      />
    </div>
    <div class="lyrics-spacer-bottom" />
  </div>
</template>

<script setup>
/**
 * ChordLyricsView — 歌词滚动容器，活动行垂直居中。
 *
 * # 滚动策略
 * - 使用真实滚动容器（overflow-y:auto），换行/翻译行高度自适应。
 * - activeLineIndex 变化时，用 anime.js spring 平滑滚动活动行到视口中心。
 * - 隐藏滚动条（scrollbar-width:none），对齐 AMLL 观感。
 *
 * # 性能优化
 * - 仅活动行接收实时 nowMs（唯一需要逐字填充的行），其余行传 0 常量，
 *   避免整列表 60fps 重渲染。活动行每天只重渲染 1 个组件。
 */
import { ref, watch, onMounted, nextTick } from 'vue'
import { useAnime } from '@/composables/useAnime.js'
import ChordLyricLine from './ChordLyricLine.vue'

const props = defineProps({
  lyricLines: { type: Array, default: () => [] },
  activeLineIndex: { type: Number, default: -1 },
  showTranslation: { type: Boolean, default: true },
  fontSizePx: { type: Number, default: 30 },
  fontWeight: { type: Number, default: 600 },
  letterSpacing: { type: String, default: 'normal' },
  enableBlur: { type: Boolean, default: true },
  enableScale: { type: Boolean, default: true },
  enableSpring: { type: Boolean, default: true },
})

const emit = defineEmits(['line-click', 'background-click'])

const scrollEl = ref(null)
// 行 DOM 引用（按索引维护，el 为 null 时清理，避免卸载残留过期引用）
const lineEls = ref([])
let scrollAnim = null

const { animate } = useAnime()

function setLineRef(el, i) {
  // 按索引占位；卸载时 el 为 null，置空槽位以释放引用，同时保持数组长度稳定
  lineEls.value[i] = el || null
}

function scrollToActive(immediate = false) {
  const container = scrollEl.value
  if (!container) return
  const idx = props.activeLineIndex
  if (idx < 0 || idx >= lineEls.value.length) return
  const lineEl = lineEls.value[idx]
  if (!lineEl) return

  const target = lineEl.offsetTop - container.clientHeight / 2 + lineEl.offsetHeight / 2
  const clamped = Math.max(0, Math.min(target, container.scrollHeight - container.clientHeight))

  if (scrollAnim) scrollAnim.cancel?.()
  scrollAnim = animate(container, {
    scrollTop: clamped,
    duration: immediate ? 0 : 320,
    ease: 'spring({ bounce: 0.2, duration: 320 })',
  })
}

function onLineClick(line) {
  emit('line-click', line)
}

function onBackgroundClick() {
  emit('background-click')
}

// 活动行变化 → 平滑滚动居中
watch(
  () => props.activeLineIndex,
  (idx) => {
    if (idx < 0) return
    nextTick(() => scrollToActive(false))
  },
)

// 歌词列表变化 → 立即定位到当前行
watch(
  () => props.lyricLines,
  () => {
    lineEls.value = []
    nextTick(() => scrollToActive(true))
  },
)

onMounted(() => {
  if (props.lyricLines.length > 0) {
    nextTick(() => scrollToActive(true))
  }
})
</script>

<style scoped>
.chord-lyrics-view {
  position: relative;
  width: 100%;
  height: 100%;
  overflow-y: auto;
  overflow-x: hidden;
  scrollbar-width: none;
  -ms-overflow-style: none;
  padding: 0 24px;
  box-sizing: border-box;
  /* 对齐 AMLL：限制绘制边界，避免滚动时触发全局重排 */
  contain: paint;
  -webkit-mask-image: linear-gradient(
    to bottom,
    transparent 0%,
    #000 12%,
    #000 88%,
    transparent 100%
  );
  mask-image: linear-gradient(
    to bottom,
    transparent 0%,
    #000 12%,
    #000 88%,
    transparent 100%
  );
}

.chord-lyrics-view::-webkit-scrollbar {
  display: none;
}

.lyrics-spacer-top {
  height: 42vh;
}

.lyrics-spacer-bottom {
  height: 42vh;
}

.lyrics-line-wrap {
  padding: 0 8px;
}
</style>