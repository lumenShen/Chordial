<template>
  <div class="chord-music-info">
    <div v-if="showMusicName" class="mi-title" :title="title">{{ title }}</div>
    <div v-if="showMusicArtists" class="mi-artists" :title="artistsText">
      <!-- 每位歌手独立链接，点击跳转歌手页（对齐 AMLL onArtistClicked） -->
      <template v-for="(artist, i) in artistList" :key="i">
        <a
          class="mi-artist"
          :href="artistHref(artist)"
          @click.prevent="onArtistClick(artist)"
        >{{ artist.name }}</a>
        <span v-if="i < artistList.length - 1" class="mi-artist-sep">{{ artistSeparator(i) }}</span>
      </template>
    </div>
    <div v-if="showMusicAlbum && album" class="mi-album">{{ album }}</div>
  </div>
</template>

<script setup>
/**
 * ChordMusicInfo — 歌曲信息（标题/艺人/专辑）。
 * 显示项由设置切换（showMusicName / showMusicArtists / showMusicAlbum）。
 * 艺人以独立链接渲染，点击跳转歌手详情页（对齐 AMLL MusicInfo 的 onArtistClicked）。
 */
import { computed } from 'vue'
import { useRouter } from 'vue-router'

const props = defineProps({
  title: { type: String, default: '未知歌曲' },
  artists: { type: String, default: '未知歌手' },
  artistList: { type: Array, default: () => [] },
  album: { type: String, default: '' },
  showMusicName: { type: Boolean, default: true },
  showMusicArtists: { type: Boolean, default: true },
  showMusicAlbum: { type: Boolean, default: false },
})

const router = useRouter()

// 无 artistList 时回退到纯文本（artists 字符串）
const artistsText = computed(() => props.artists || '未知歌手')

function artistHref(artist) {
  return artist?.id ? `/artists/${artist.id}` : '#'
}

function artistSeparator(i) {
  // 对齐 AMLL：倒数第二个用 " & "，其余用 ", "
  return i === props.artistList.length - 2 ? ' & ' : ', '
}

function onArtistClick(artist) {
  if (artist?.id) router.push(`/artists/${artist.id}`)
}
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

.mi-artist {
  color: rgba(255, 255, 255, 0.75);
  text-decoration: none;
  cursor: pointer;
  transition: opacity 0.2s;
}

.mi-artist:hover {
  opacity: 0.75;
  text-decoration: underline;
}

.mi-artist-sep {
  opacity: 0.6;
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