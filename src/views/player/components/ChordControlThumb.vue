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
      <div v-if="showQuality" class="ct-quality" title="当前音质">
        {{ qualityLabel }}
      </div>
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

const emit = defineEmits(['close', 'open-menu'])

const showQuality = computed(() => !!props.qualityLabel)

function onClose() { emit('close') }
function onOpenMenu() { emit('open-menu') }
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

.ct-quality {
  font-size: 11px;
  color: rgba(255, 255, 255, 0.7);
  background: rgba(255, 255, 255, 0.12);
  padding: 3px 8px;
  border-radius: 10px;
  white-space: nowrap;
}
</style>