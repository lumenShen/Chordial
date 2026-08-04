<template>
  <div class="chord-music-info">
    <div v-if="showMusicName" class="mi-title" :title="title">{{ title }}</div>
    <div v-if="showMusicArtists" class="mi-artists" :title="artists">{{ artists }}</div>
    <div v-if="showMusicAlbum && album" class="mi-album">{{ album }}</div>
  </div>
</template>

<script setup>
/**
 * ChordMusicInfo — 歌曲信息（标题/艺人/专辑）。
 * 显示项由设置切换（showMusicName / showMusicArtists / showMusicAlbum）。
 */
const props = defineProps({
  title: { type: String, default: '未知歌曲' },
  artists: { type: String, default: '未知歌手' },
  album: { type: String, default: '' },
  showMusicName: { type: Boolean, default: true },
  showMusicArtists: { type: Boolean, default: true },
  showMusicAlbum: { type: Boolean, default: false },
})
</script>

<style scoped>
.chord-music-info {
  display: flex;
  flex-direction: column;
  min-width: 0;
  line-height: 1.3;
  text-align: left;
}

.mi-title {
  font-size: 15px;
  font-weight: 600;
  color: #fff;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.mi-artists {
  font-size: 12px;
  color: rgba(255, 255, 255, 0.75);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  margin-top: 1px;
}

.mi-album {
  font-size: 11px;
  color: rgba(255, 255, 255, 0.55);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  margin-top: 1px;
}
</style>