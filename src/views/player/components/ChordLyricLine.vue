<template>
  <div
    class="chord-lyric-line"
    :class="{
      'is-active': isActive,
      'is-before': isBefore,
      'is-after': isAfter,
      'no-blur': !enableBlur,
      'no-scale': !enableScale,
    }"
    :style="lineStyle"
    @click="onClick"
  >
    <!-- 活动行背景光晕（对齐 AMLL bgWrapper 观感） -->
    <div class="lyric-line-glow" aria-hidden="true" />

    <!-- 主歌词：整行逐字同步填充（对齐 AMLL 逐字高亮） -->
    <div class="lyric-main">
      <template v-for="(word, wi) in line.words" :key="wi">
        <span class="lyric-word">
          <!-- 底层静态色（未唱到部分） -->
          <span class="word-text">{{ word.word }}</span>
          <!-- 顶层高亮填充（已唱到部分，background-clip:text） -->
          <span
            class="word-fill"
            :style="{ width: wordFillPct(wi, word) + '%' }"
          >{{ word.word }}</span>
        </span>
      </template>
    </div>

    <!-- 翻译行 -->
    <div v-if="showTranslation && line.translatedLyric" class="lyric-translation">
      {{ line.translatedLyric }}
    </div>
  </div>
</template>

<script setup>
/**
 * ChordLyricLine — 单行歌词，整行逐字同步填充高亮。
 *
 * # 高亮实现（对齐 AMLL 逐字歌词）
 * - 每个词渲染两个 span：底层 `.word-text`（静态暗色）+ 顶层 `.word-fill`（亮白）。
 * - 对每个词按"它是否已被唱到"计算填充宽度：
 *     · 已唱完的词 → 100%
 *     · 正在唱的词 → 按 (now - start)/(end - start) 扫过
 *     · 未唱到的词 → 0%
 * - `.word-fill` 用 `background-clip:text` + 超宽 `background-image` 实现
 *   多词共用同一整行渐变，宽度的变化即"墨水填充"效果。
 * - 活动行放大提亮；前后行缩小变暗模糊。
 */
import { computed, inject, ref } from 'vue'
import { CHORD_NOW_MS } from '../composables/injectionKeys.js'

const props = defineProps({
  line: { type: Object, required: true },
  isActive: { type: Boolean, default: false },
  isBefore: { type: Boolean, default: false },
  isAfter: { type: Boolean, default: false },
  showTranslation: { type: Boolean, default: true },
  fontSizePx: { type: Number, default: 30 },
  fontWeight: { type: Number, default: 600 },
  letterSpacing: { type: String, default: 'normal' },
  // AMLL 效果开关
  enableBlur: { type: Boolean, default: true },
  enableScale: { type: Boolean, default: true },
  enableSpring: { type: Boolean, default: true },
})

const emit = defineEmits(['click'])

// 注入 60fps 播放进度。仅活动行读取（非活动行短路为 0，不订阅），
// 因此只有活动行每帧重渲染，其余行完全不受高频更新影响。
const nowMs = inject(CHORD_NOW_MS, ref(0))
const activeNowMs = computed(() => (props.isActive ? nowMs.value : 0))

// 单个词填充百分比 0-100
function wordFillPct(wordIndex, word) {
  if (!props.isActive) return 0
  const t = activeNowMs.value
  const start = word.startTime
  const end = word.endTime || start + 200
  if (t >= end) return 100
  if (t < start) return 0
  const span = end - start
  if (span <= 0) return 100
  return Math.min(100, Math.max(0, ((t - start) / span) * 100))
}

const lineStyle = computed(() => ({
  fontSize: props.fontSizePx + 'px',
  fontWeight: props.fontWeight,
  letterSpacing: props.letterSpacing,
  // 弹簧动画开启时，过渡使用更长时长 + 弹性 cubic-bezier（近似 spring 观感）
  ...(props.enableSpring
    ? { transitionDuration: '0.5s', transitionTimingFunction: 'cubic-bezier(0.34, 1.56, 0.64, 1)' }
    : {}),
}))

function onClick() {
  emit('click', props.line)
}
</script>

<style scoped>
.chord-lyric-line {
  padding: 6px 0;
  cursor: pointer;
  user-select: none;
  text-align: center;
  opacity: 0.45;
  color: rgba(255, 255, 255, 0.55);
  line-height: 1.4;
  transition: color 0.3s, opacity 0.3s, filter 0.3s, transform 0.3s;
  will-change: transform, opacity;
}

/* 活动行：放大 + 提亮（scale 可关闭） */
.chord-lyric-line.is-active {
  opacity: 1;
  color: #ffffff;
  transform: scale(1.05);
}

/* 无缩放效果时活动行保持原大小 */
.chord-lyric-line.no-scale.is-active {
  transform: none;
}

/* 前后行：模糊 + 更暗（blur 可关闭） */
.chord-lyric-line.is-before,
.chord-lyric-line.is-after {
  filter: blur(1.5px);
  opacity: 0.35;
}

/* 无模糊效果时隐藏模糊 */
.chord-lyric-line.no-blur.is-before,
.chord-lyric-line.no-blur.is-after {
  filter: none;
  opacity: 0.45;
}

.lyric-main {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  align-items: baseline;
  gap: 2px;
}

/* 词容器：相对定位，fill 叠加其上 */
.lyric-word {
  position: relative;
  display: inline-block;
  white-space: pre;
}

.word-text {
  display: inline-block;
}

/* 高亮填充层：background-clip:text 实现"墨水填充"。
   为让整行多个词共享同一横向渐变，background-image 设为超宽（3000px），
   白+透明渐变；宽度变化即填充进度。 */
.word-fill {
  position: absolute;
  inset: 0;
  overflow: hidden;
  white-space: pre;
  color: #ffffff;
  -webkit-background-clip: text;
  background-clip: text;
  background-image: linear-gradient(
    90deg,
    #ffffff 0%,
    #f0f6ff 100%
  );
  /* 触摸时禁用文字选择，避免闪选 */
  -webkit-user-select: none;
  user-select: none;
}

/* 翻译行 */
.lyric-translation {
  font-size: 0.55em;
  font-weight: 400;
  opacity: 0.85;
  margin-top: 2px;
  color: rgba(255, 255, 255, 0.72);
}
</style>