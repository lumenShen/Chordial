<template>
  <div ref="rootEl" class="amll-vertical-layout" :class="{ 'hide-lyric': hideLyric }">
    <!-- 顶部拖拽区 -->
    <div class="v-drag-area" />

    <!-- 顶部控制条 -->
    <div class="v-thumb">
      <ChordControlThumb
        :title="title"
        :artists="artists"
        :artist-list="artistList"
        :album="album"
        :quality-label="qualityLabel"
        :show-music-name="showMusicName"
        :show-music-artists="showMusicArtists"
        :show-music-album="showMusicAlbum"
        :is-immersive="isImmersive"
        @close="onClose"
        @open-menu="onOpenMenu"
      />
    </div>

    <!-- 歌词主视图（歌词可见时） -->
    <div class="v-lyric-layout">
      <div ref="phonySmall" class="v-phony-small-cover" />
      <div class="v-small-controls">
        <ChordMusicInfo
          :title="title"
          :artists="artists"
          :artist-list="artistList"
          :album="album"
          :show-music-name="showMusicName"
          :show-music-artists="showMusicArtists"
          :show-music-album="showMusicAlbum"
        />
      </div>
      <div class="v-lyric">
        <ChordLyricsView
          :lyric-lines="lyricLines"
          :active-line-index="activeLineIndex"
          :show-translation="showTranslation"
          :font-size-px="fontSizePx"
          :font-weight="fontWeight"
          :letter-spacing="letterSpacing"
          :font-family="fontFamily"
          :enable-blur="enableBlur"
          :enable-scale="enableScale"
          :enable-spring="enableSpring"
          :align-position="0.1"
          @line-click="onLineClick"
          @background-click="onBackgroundClick"
        />
      </div>
    </div>

    <!-- 无歌词视图（隐藏歌词时） -->
    <div class="v-no-lyric-layout">
      <div ref="phonyBig" class="v-phony-big-cover" />
      <div class="v-big-controls">
        <ChordSideControls
          :title="title"
          :artists="artists"
          :artist-list="artistList"
          :album="album"
          :show-music-name="showMusicName"
          :show-music-artists="showMusicArtists"
          :show-music-album="showMusicAlbum"
          :duration-ms="durationMs"
          :buffer-ms="bufferMs"
          :formatted-current-time="formattedCurrentTime"
          :formatted-duration="formattedDuration"
          :formatted-remaining="formattedRemaining"
          :is-playing="isPlaying"
          :is-loading="isLoading"
          :is-shuffle="isShuffle"
          :is-repeat-all="isRepeatAll"
          :is-repeat-one="isRepeatOne"
          :volume="volume"
          :muted="muted"
          :show-volume-control="showVolumeControl"
          :show-remaining-time="showRemainingTime"
          :is-immersive="isImmersive"
          @seek="onSeek"
          @toggle-play="onTogglePlay"
          @prev="onPrev"
          @next="onNext"
          @toggle-shuffle="onToggleShuffle"
          @cycle-repeat="onCycleRepeat"
          @toggle-mute="onToggleMute"
          @volume="onVolume"
          @toggle-remaining="onToggleRemaining"
        />
      </div>
    </div>

    <!-- 封面帧：跨小/大位置 FLIP 动画 -->
    <div
      v-if="coverStyle"
      ref="coverFrameRef"
      class="v-cover-frame"
      :class="{ 'immerse': immerseCover }"
      :style="coverStyle"
    >
      <ChordCoverPanel :cover-url="coverUrl" :music-paused="!isPlaying" />
    </div>
  </div>
</template>

<script setup>
/**
 * ChordVerticalLayout — 竖屏布局（对齐 AMLL VerticalLayout）。
 *
 * # 结构
 * - thumb（顶部控制条）
 * - lyricLayout：隐藏歌词前 → 小封面(占位测量) + 歌曲信息 + 歌词
 * - noLyricLayout：隐藏歌词时 → 大封面(占位测量) + 大控件
 * - coverFrame：绝对定位封面，跨 小/大 位置 FLIP 动画（spring）
 *
 * # 封面 FLIP
 * 对齐 AMLL calculateCoverLayout：用两个不可见的 phony 封面占位测量目标位置，
 * 封面帧通过 anime.js spring 在 hideLyric 切换时平滑移动到目标矩形。
 */
import { ref, watch, onMounted, onBeforeUnmount, nextTick } from 'vue'
import ChordControlThumb from './ChordControlThumb.vue'
import ChordMusicInfo from './ChordMusicInfo.vue'
import ChordSideControls from './ChordSideControls.vue'
import ChordCoverPanel from './ChordCoverPanel.vue'
import ChordLyricsView from './ChordLyricsView.vue'
import { useAnime } from '@/composables/useAnime.js'

const props = defineProps({
  title: String, artists: String, artistList: Array, album: String,
  qualityLabel: String, coverUrl: String,
  showMusicName: Boolean, showMusicArtists: Boolean, showMusicAlbum: Boolean,
  lyricLines: Array, activeLineIndex: Number,
  showTranslation: Boolean, fontSizePx: Number, fontWeight: Number,
  letterSpacing: String, fontFamily: String,
  enableBlur: Boolean, enableScale: Boolean, enableSpring: Boolean,
  durationMs: Number, bufferMs: Number,
  formattedCurrentTime: String, formattedDuration: String, formattedRemaining: String,
  isPlaying: Boolean, isLoading: Boolean,
  isShuffle: Boolean, isRepeatAll: Boolean, isRepeatOne: Boolean,
  volume: Number, muted: Boolean,
  showVolumeControl: Boolean, showRemainingTime: Boolean,
  isImmersive: Boolean,
  hideLyric: { type: Boolean, default: false },
  immerseCover: { type: Boolean, default: false },
})

const emit = defineEmits([
  'close', 'open-menu', 'seek', 'toggle-play', 'prev', 'next',
  'toggle-shuffle', 'cycle-repeat', 'toggle-mute', 'volume',
  'toggle-remaining', 'line-click', 'background-click',
])

const rootEl = ref(null)
const phonySmall = ref(null)
const phonyBig = ref(null)
const coverFrameRef = ref(null)
const coverStyle = ref(null)

const { animate } = useAnime()

let ro = null
let coverAnim = null
const hideLyricRef = { current: props.hideLyric }
const immerseCoverRef = { current: props.immerseCover }

/** 对齐 AMLL calculateCoverLayout：计算封面帧的目标矩形 */
function calcCoverLayout(hideLyricNow = hideLyricRef.current) {
  const root = rootEl.value
  if (!root) return null
  const target = hideLyricNow ? phonyBig.value : phonySmall.value
  if (!target) return null
  const rootB = root.getBoundingClientRect()
  const tB = target.getBoundingClientRect()
  if (rootB.height <= 0) return null
  if (immerseCoverRef.current) {
    const halfHeight = tB.top - rootB.top + tB.height / 2
    const size = Math.max(halfHeight * 2.4, rootB.width * 1.2, Math.min(target.clientWidth, target.clientHeight))
    return {
      width: `${size}px`,
      height: `${size}px`,
      left: `${tB.x - rootB.x + (tB.width - size) / 2}px`,
      top: `${tB.y - rootB.y + (tB.height - size) / 2}px`,
    }
  }
  const size = Math.min(tB.width, tB.height)
  return {
    width: `${size}px`,
    height: `${size}px`,
    left: `${tB.x - rootB.x + (tB.width - size) / 2}px`,
    top: `${tB.y - rootB.y + (tB.height - size) / 2}px`,
  }
}

function applyCoverStyle(animateNow = false) {
  const next = calcCoverLayout()
  if (!next) return
  if (!animateNow || !coverStyle.value) {
    coverStyle.value = next
    return
  }
  // FLIP：从当前位置动画到目标（对齐 AMLL spring: stiffness 200, damping 30）
  if (coverAnim) coverAnim.cancel?.()
  coverAnim = animate(coverFrameRef.value, {
    ...next,
    duration: 500,
    ease: 'spring({ stiffness: 200, damping: 30 })',
  })
  coverStyle.value = next
}

watch(
  () => props.hideLyric,
  (v) => {
    hideLyricRef.current = v
    nextTick(() => applyCoverStyle(true))
  },
)

watch(
  () => props.immerseCover,
  (v) => {
    immerseCoverRef.current = v
    nextTick(() => applyCoverStyle(true))
  },
)

onMounted(() => {
  hideLyricRef.current = props.hideLyric
  immerseCoverRef.current = props.immerseCover
  nextTick(() => {
    applyCoverStyle(false)
    ro = new ResizeObserver(() => applyCoverStyle(false))
    if (phonySmall.value) ro.observe(phonySmall.value)
    if (phonyBig.value) ro.observe(phonyBig.value)
  })
})

onBeforeUnmount(() => {
  ro?.disconnect()
  coverAnim?.cancel?.()
})

function onClose() { emit('close') }
function onOpenMenu() { emit('open-menu') }
function onSeek(ms) { emit('seek', ms) }
function onTogglePlay() { emit('toggle-play') }
function onPrev() { emit('prev') }
function onNext() { emit('next') }
function onToggleShuffle() { emit('toggle-shuffle') }
function onCycleRepeat() { emit('cycle-repeat') }
function onToggleMute() { emit('toggle-mute') }
function onVolume(v) { emit('volume', v) }
function onToggleRemaining() { emit('toggle-remaining') }
function onLineClick(line) { emit('line-click', line) }
function onBackgroundClick() { emit('background-click') }
</script>

<style scoped>
.amll-vertical-layout {
  position: relative;
  width: 100%;
  height: 100%;
  min-width: 0;
  min-height: 0;
  display: grid;
  grid-template-rows:
    [drag-area] 30px
    [thumb] auto
    [main-view] 1fr
    [control] 0fr;
  gap: 2px;
  overflow: hidden;
}

.v-drag-area {
  -webkit-app-region: drag;
  grid-row: drag-area;
}

.v-thumb {
  z-index: 1;
  grid-row: thumb;
  justify-self: center;
}

@media screen and (width <= 768px) {
  .v-thumb {
    margin: 4px;
  }
}

/* 歌词主视图 */
.v-lyric-layout {
  grid-area: main-view / 1 / main-view / 2;
  display: grid;
  grid-template-rows: 1em [controls] 0fr [lyric-view] 1fr [bottom-controls] 0fr;
  grid-template-columns: 3em [cover-side] 0fr [info-side] 1fr 3em;
}

@media screen and (width <= 480px) {
  .v-lyric-layout {
    grid-template-columns: 20px [cover-side] 0fr [info-side] 1fr 20px;
  }
}

.v-phony-small-cover {
  aspect-ratio: 1;
  opacity: 0;
  pointer-events: none;
  grid-area: controls / cover-side;
  place-self: center;
  width: 6em;
}

@media screen and (width <= 480px) {
  .v-phony-small-cover {
    width: 4.5em;
  }
}

.v-small-controls {
  z-index: 1;
  grid-area: controls / info-side;
  align-self: center;
  min-width: 0;
  height: fit-content;
  padding-left: 1em;
  transition: opacity 0.25s 0.25s;
  overflow: visible;
}

.v-lyric {
  opacity: 1;
  box-sizing: content-box;
  grid-area: lyric-view / 1 / lyric-view / 5;
  min-height: 0;
  transition: opacity 0.5s 0.5s;
  mask-image: linear-gradient(#0000 0%, #000 10% 100%);
}

/* 无歌词视图 */
.v-no-lyric-layout {
  pointer-events: none;
  grid-area: main-view / 1 / main-view / 2;
  display: grid;
  grid-template-rows: 1em [cover-view] 1fr [lyric-view] 0fr;
  grid-template-columns: 3em [main-view] 1fr 3em;
  overflow-y: hidden;
}

@media screen and (width <= 480px) {
  .v-no-lyric-layout {
    grid-template-columns: 2em [main-view] 1fr 2em;
  }
}

.v-phony-big-cover {
  opacity: 0;
  pointer-events: none;
  grid-area: cover-view / 2 / cover-view / 3;
}

.v-big-controls {
  opacity: 0;
  z-index: 2;
  grid-area: lyric-view / 2 / lyric-view / 3;
  min-width: 0;
  transition: opacity 0.5s;
}

/* 封面帧 */
.v-cover-frame {
  position: absolute;
  z-index: 1;
  top: 0;
  left: 0;
}

.v-cover-frame.immerse {
  mask-image: linear-gradient(#000 0% 70%, #0000 100%);
}

/* 隐藏歌词 */
.amll-vertical-layout.hide-lyric .v-small-controls {
  opacity: 0;
  pointer-events: none;
  transition: opacity 0.5s;
}

.amll-vertical-layout.hide-lyric .v-no-lyric-layout {
  pointer-events: auto;
}

.amll-vertical-layout.hide-lyric .v-lyric {
  opacity: 0;
  pointer-events: none;
  transition: opacity 0.5s;
}

.amll-vertical-layout.hide-lyric .v-big-controls {
  opacity: 1;
}
</style>