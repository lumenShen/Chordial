<template>
  <div ref="rootEl" class="amll-horizontal-layout" :class="{ 'hide-lyric': hideLyric }">
    <!-- 顶部拖拽区（可拖动窗口） -->
    <div class="h-drag-area" />

    <!-- 顶部控制横条 -->
    <div class="h-thumb">
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

    <!-- 封面（左侧列，垂直居中） -->
    <div ref="coverRef" class="h-cover">
      <ChordCoverPanel :cover-url="coverUrl" :music-paused="!isPlaying" />
    </div>

    <!-- 左侧列：歌曲信息 + 进度 + 播放控制 + 音量 -->
    <div class="h-controls">
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

    <!-- 右侧列：歌词（对齐到封面中心） -->
    <div class="h-lyric">
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
        :align-position="alignPosition"
        @line-click="onLineClick"
        @background-click="onBackgroundClick"
      />
    </div>

    <!-- 底部行：切换按钮（歌词/封面、播放队列、投放） -->
    <div v-if="showBottomControls" class="h-bottom-controls">
      <button class="h-toggle-btn" title="播放队列" @click="onOpenQueue">
        <HugeiconsIcon :icon="Queue01Icon" :size="20" />
      </button>
      <button class="h-toggle-btn" :title="hideLyric ? '显示歌词' : '隐藏歌词'" @click="onToggleLyric">
        <HugeiconsIcon :icon="CaptionsIcon" :size="20" />
      </button>
      <span class="h-bottom-spacer" />
      <button class="h-toggle-btn" title="投放" @click="onAirplay">
        <HugeiconsIcon :icon="AirplayIcon" :size="20" />
      </button>
    </div>
  </div>
</template>

<script setup>
/**
 * ChordHorizontalLayout — 横屏布局（对齐 AMLL HorizontalLayout）。
 *
 * # 结构（CSS Grid 两列）
 * - 左列（info-side，0.45fr）：thumb（顶部控制条）→ cover（封面，垂直居中）→ controls（信息+进度+播放）
 * - 右列（player-side，0.55fr）：lyric（歌词，跨第 2~5 行，右侧留白）
 * - 底部行（bottom-controls）：[播放队列, 歌词/封面切换, 弹性空白, 投放]
 *
 * # 歌词对齐
 * 活动行对齐到封面中心的垂直位置（alignPosition），由 ResizeObserver 实时计算，
 * 对齐 AMLL 的 setAlignPosition((coverB.top + coverB.height/2 - layoutB.top) / layoutB.height)。
 */
import { ref, onMounted, onBeforeUnmount } from 'vue'
import { HugeiconsIcon } from '@hugeicons/vue'
import { Queue01Icon, AirplayIcon, CaptionsIcon } from '@hugeicons/core-free-icons'
import ChordControlThumb from './ChordControlThumb.vue'
import ChordCoverPanel from './ChordCoverPanel.vue'
import ChordSideControls from './ChordSideControls.vue'
import ChordLyricsView from './ChordLyricsView.vue'

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
  showBottomControls: Boolean,
  isImmersive: Boolean,
  hideLyric: { type: Boolean, default: false },
})

const emit = defineEmits([
  'close', 'open-menu', 'seek', 'toggle-play', 'prev', 'next',
  'toggle-shuffle', 'cycle-repeat', 'toggle-mute', 'volume',
  'toggle-remaining', 'line-click', 'background-click', 'open-queue',
  'toggle-lyric', 'airplay',
])

const rootEl = ref(null)
const coverRef = ref(null)
// 歌词对齐位置：封面中心在布局中的垂直比例（初始 0.25，对齐 AMLL）
const alignPosition = ref(0.25)

let ro = null

function updateAlign() {
  const root = rootEl.value
  const cover = coverRef.value
  if (!root || !cover) return
  const rootB = root.getBoundingClientRect()
  const coverB = cover.getBoundingClientRect()
  if (rootB.height <= 0) return
  alignPosition.value = (coverB.top + coverB.height / 2 - rootB.top) / rootB.height
}

onMounted(() => {
  updateAlign()
  ro = new ResizeObserver(() => updateAlign())
  if (rootEl.value) ro.observe(rootEl.value)
  if (coverRef.value) ro.observe(coverRef.value)
})

onBeforeUnmount(() => ro?.disconnect())

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
function onOpenQueue() { emit('open-queue') }
function onToggleLyric() { emit('toggle-lyric') }
function onAirplay() { emit('airplay') }
</script>

<style scoped>
.amll-horizontal-layout {
  --hide-lyric-left: 61.1111%;
  --horizontal-layout-max-width: min(50vh, 38vw);
  position: relative;
  width: 100%;
  height: 100%;
  display: grid;
  grid-template-rows:
    [drag-area] minmax(30px, 0.45fr)
    [thumb] auto
    [cover] auto
    [music-info] 3fr
    [bottom-controls] 0fr
    0.3fr;
  grid-template-columns:
    [info-side] 0.45fr
    [player-side] 0.55fr
    [side-controls] 0fr;
  gap: 8px;
  transition: all 0.5s ease-in-out;
}

@media screen and (height <= 1000px) {
  .amll-horizontal-layout {
    --horizontal-layout-max-width: min(45vh, 38vw);
  }
}

@media screen and (height <= 768px) {
  .amll-horizontal-layout {
    gap: 2px;
    font-size: 0.8em;
  }
}

/* 顶部拖拽区（横跨，可拖动窗口） */
.h-drag-area {
  -webkit-app-region: drag;
  grid-row: drag-area;
  grid-column: 1 / -1;
}

/* 顶部控制条 */
.h-thumb {
  grid-area: thumb / info-side;
  justify-self: center;
  margin: 2vh;
  transition: left 0.5s cubic-bezier(0.5, 0, 0.5, 1);
  left: 0%;
  position: relative;
}

/* 封面：左侧列、垂直居中，尺寸由 --horizontal-layout-max-width 控制 */
.h-cover {
  grid-area: cover / info-side;
  place-self: center;
  width: var(--horizontal-layout-max-width);
  height: var(--horizontal-layout-max-width);
  aspect-ratio: 1;
  position: relative;
  transition: left 0.5s cubic-bezier(0.5, 0, 0.5, 1);
  left: 0%;
}

/* 左侧列：信息 + 进度 + 播放控制 + 音量 */
.h-controls {
  grid-area: music-info / info-side;
  justify-self: center;
  width: var(--horizontal-layout-max-width);
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  margin-top: calc(1.75em - 8px);
  transition: left 0.5s cubic-bezier(0.5, 0, 0.5, 1);
  left: 0%;
  position: relative;
}

/* 右侧列：歌词 */
.h-lyric {
  grid-area: 2 / player-side / 5;
  width: 100%;
  height: 100%;
  box-sizing: border-box;
  padding-right: 15%;
  opacity: 1;
  transition: opacity 0.5s cubic-bezier(0.5, 0, 0.5, 1) 0.25s;
}

@media screen and (width <= 1600px), (height <= 1000px) {
  .h-lyric {
    padding-right: 8%;
  }
}

/* 底部行：切换按钮 */
.h-bottom-controls {
  grid-area: bottom-controls / 1 / bottom-controls / 4;
  display: flex;
  flex-direction: row-reverse;
  align-items: center;
  gap: 4em;
  padding-left: 4em;
  padding-right: 4em;
}

@media screen and (width <= 1600px), (height <= 1000px) {
  .h-bottom-controls {
    gap: 2em;
    padding-left: 2em;
    padding-right: 2em;
  }
}

.h-toggle-btn {
  aspect-ratio: 1;
  width: 3.5vh;
  background: rgba(255, 255, 255, 0.08);
  border: none;
  border-radius: 50%;
  color: #fff;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  transition: background 0.2s, transform 0.1s;
}

.h-toggle-btn:hover {
  background: rgba(255, 255, 255, 0.18);
}

.h-toggle-btn:active {
  transform: scale(0.92);
}

.h-bottom-spacer {
  flex: 1;
}

/* 隐藏歌词：右侧歌词淡出，左列整体右移居中 */
.amll-horizontal-layout.hide-lyric .h-lyric {
  opacity: 0;
  pointer-events: none;
  transition: opacity 0.25s cubic-bezier(0.5, 0, 0.5, 1), transform 0.5s cubic-bezier(0.5, 0, 0.5, 1);
}

.amll-horizontal-layout.hide-lyric .h-thumb,
.amll-horizontal-layout.hide-lyric .h-cover,
.amll-horizontal-layout.hide-lyric .h-controls {
  left: var(--hide-lyric-left);
}
</style>