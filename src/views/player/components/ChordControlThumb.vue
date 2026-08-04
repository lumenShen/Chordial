<template>
  <div class="chord-control-thumb" :class="{ 'is-immersive': isImmersive }">
    <button
      class="ct-btn ct-close"
      :class="{ disabled: isImmersive }"
      :disabled="isImmersive"
      title="关闭"
      @click="onClose"
    >
      <HugeiconsIcon :icon="Cancel01Icon" :size="20" />
    </button>

    <div class="ct-info">
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

    <div class="ct-right">
      <Transition name="quality">
        <div
          v-if="showQuality"
          class="ct-quality"
          :class="{ clickable: hasQualityClick }"
          :title="qualityLabel"
          @click="onQualityClick"
        >
          {{ qualityLabel }}
        </div>
      </Transition>
      <button class="ct-btn" title="打开菜单" @click="onOpenMenu">
        <HugeiconsIcon :icon="MoreHorizontalIcon" :size="20" />
      </button>
    </div>
  </div>
</template>

<script setup>
/**
 * ChordControlThumb — 顶部控制横条（横屏布局）。
 * 关闭按钮 + 歌曲信息 + 音质标签 + 菜单按钮。
 * 音质标签对齐 AMLL AudioQualityTag：进入/离开动画 + hover/active 缩放 + 可点击。
 */
import { computed } from 'vue'
import { HugeiconsIcon } from '@hugeicons/vue'
import { Cancel01Icon, MoreHorizontalIcon } from '@hugeicons/core-free-icons'
import ChordMusicInfo from './ChordMusicInfo.vue'

const props = defineProps({
  title: { type: String, default: '未知歌曲' },
  artists: { type: String, default: '未知歌手' },
  artistList: { type: Array, default: () => [] },
  album: { type: String, default: '' },
  qualityLabel: { type: String, default: '' },
  showMusicName: { type: Boolean, default: true },
  showMusicArtists: { type: Boolean, default: true },
  showMusicAlbum: { type: Boolean, default: false },
  isImmersive: { type: Boolean, default: false },
})

const emit = defineEmits(['close', 'open-menu', 'quality-click'])

const showQuality = computed(() => !!props.qualityLabel)
const hasQualityClick = computed(() => !!props.qualityLabel)

function onClose() { emit('close') }
function onOpenMenu() { emit('open-menu') }
function onQualityClick() { emit('quality-click') }
</script>

<style scoped>
.chord-control-thumb {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 16px;
  -webkit-app-region: drag;
  user-select: none;
  transition: opacity 0.3s, transform 0.3s;
}

.chord-control-thumb.is-immersive {
  opacity: 0;
  pointer-events: none;
  transform: translateY(-8px);
}

.ct-btn {
  -webkit-app-region: no-drag;
  background: none;
  border: none;
  color: rgba(255, 255, 255, 0.85);
  cursor: pointer;
  padding: 6px;
  border-radius: 8px;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: background 0.2s, color 0.2s, opacity 0.2s;
}

.ct-btn:hover {
  background: rgba(255, 255, 255, 0.12);
  color: #fff;
}

.ct-btn.disabled {
  opacity: 0.15;
  pointer-events: none;
}

.ct-info {
  flex: 1;
  min-width: 0;
}

.ct-right {
  display: flex;
  align-items: center;
  gap: 8px;
  -webkit-app-region: no-drag;
}

/* 音质标签：对齐 AMLL AudioQualityTag 的胶囊样式（白字、细描边、hover/active 缩放） */
.ct-quality {
  font-size: 11px;
  font-weight: 500;
  letter-spacing: 0.018em;
  color: #fff;
  background: rgba(255, 255, 255, 0.12);
  border: 1px solid rgba(255, 255, 255, 0.25);
  padding: 2px 10px;
  border-radius: 999px;
  white-space: nowrap;
  display: inline-flex;
  align-items: center;
  user-select: none;
  transition: transform 0.2s ease, background 0.2s ease;
}

.ct-quality.clickable {
  cursor: pointer;
}

.ct-quality.clickable:hover {
  transform: scale(0.95);
}

.ct-quality.clickable:active {
  transform: scale(0.9);
}

/* 进入/离开动画：对齐 AMLL AudioQualityTag 的 show/hide（淡入 + 缩放） */
.quality-enter-active,
.quality-leave-active {
  transition: opacity 0.25s ease, transform 0.25s ease;
}

.quality-enter-from,
.quality-leave-to {
  opacity: 0;
  transform: scale(0.8);
}
</style>