<template>
  <div class="chord-music-info">
    <div v-if="showMusicName" class="mi-row" v-marquee>
      <div class="mi-row-inner" data-marquee-inner>
        <div class="mi-title" :title="title">{{ title }}</div>
      </div>
    </div>
    <div v-if="showMusicArtists" class="mi-row" v-marquee>
      <div class="mi-row-inner" data-marquee-inner>
        <div class="mi-artists" :title="artistsText">
          <!-- 每位歌手独立链接，点击跳转歌手页（对齐 AMLL MusicInfo 的艺人 <a> 结构） -->
          <template v-for="(artist, i) in artistList" :key="i">
            <a
              class="mi-artist"
              :href="artistHref(artist)"
              @click.prevent="onArtistClick(artist)"
            >{{ artist.name }}</a>
          </template>
        </div>
      </div>
    </div>
    <div v-if="showMusicAlbum && album" class="mi-row" v-marquee>
      <div class="mi-row-inner" data-marquee-inner>
        <div class="mi-album" :title="album">{{ album }}</div>
      </div>
    </div>
  </div>
</template>

<script setup>
/**
 * ChordMusicInfo — 歌曲信息（标题/艺人/专辑）。
 * 显示项由设置切换（showMusicName / showMusicArtists / showMusicAlbum）。
 * 艺人以独立 <a> 渲染，点击跳转歌手详情页（自研超集，AMLL 仅渲染样式化 <a>）。
 * 标题/艺人/专辑使用 hover 跑马灯（对齐 AMLL TextMarquee：文本溢出时滚动显示）。
 * 艺人分隔符使用 CSS 变量 --applemusic-like-lyrics-music-artists-separator（与 AMLL 一致）。
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

function onArtistClick(artist) {
  if (artist?.id) router.push(`/artists/${artist.id}`)
}

/**
 * 对齐 AMLL TextMarquee 的 hover 跑马灯：文本溢出时左右往返滚动。
 * 结构要求：指令元素为外层（overflow hidden + mask），内含 [data-marquee-inner]。
 */
const vMarquee = {
  mounted(el) {
    const inner = el.querySelector('[data-marquee-inner]')
    if (!inner) return
    let anim = null
    const enter = () => {
      anim?.cancel()
      const outerW = el.clientWidth
      const innerW = inner.scrollWidth
      if (innerW <= outerW * 0.95) return
      el.classList.add('is-marquee-animating')
      const distance = innerW - outerW * 0.95
      anim = inner.animate(
        [
          { transform: 'translateX(0px)' },
          { transform: `translateX(${-distance}px)` },
        ],
        { iterations: 2, direction: 'alternate', easing: 'linear', duration: Math.max(0, (distance * 2 / 64) * 1000) }
      )
      anim.finished.then(() => el.classList.remove('is-marquee-animating'))
    }
    const leave = () => {
      anim?.cancel()
      el.classList.remove('is-marquee-animating')
    }
    el._marqueeEnter = enter
    el._marqueeLeave = leave
    el.addEventListener('mouseenter', enter)
    el.addEventListener('mouseleave', leave)
  },
  unmounted(el) {
    el.removeEventListener('mouseenter', el._marqueeEnter)
    el.removeEventListener('mouseleave', el._marqueeLeave)
  },
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

/* 对齐 AMLL TextMarquee：外层裁剪 + 右侧渐隐 mask */
.mi-row {
  min-width: 0;
  overflow: hidden;
  -webkit-mask-image: linear-gradient(to right, #000 95%, #0000);
}

.mi-row.is-marquee-animating {
  -webkit-mask-image: linear-gradient(to left, #0000, #000 5% 95%, #0000);
}

.mi-row-inner {
  width: fit-content;
}

.mi-title {
  font-size: 15px;
  font-weight: 500;
  letter-spacing: 0.4px;
  color: #fff;
  opacity: 0.9;
  cursor: text;
  user-select: text;
  white-space: nowrap;
}

.mi-artists {
  font-size: 12px;
  font-weight: 400;
  letter-spacing: 0.4px;
  color: rgba(255, 255, 255, 0.45);
  white-space: nowrap;
  margin-top: 1px;
}

.mi-artist {
  color: inherit;
  text-decoration: none;
  cursor: pointer;
  transition: color 0.2s;
}

.mi-artist:hover {
  color: rgba(255, 255, 255, 0.75);
}

.mi-artist:active {
  color: rgba(255, 255, 255, 0.5);
}

/* 艺人分隔符：对齐 AMLL 的 :after 方案，倒数第二个用 " & "，其余用 CSS 变量 */
.mi-artist:after {
  content: var(--applemusic-like-lyrics-music-artists-separator, ', ');
}

.mi-artist:nth-last-child(2):after {
  content: ' & ';
}

.mi-artist:last-child:after {
  content: '';
}

.mi-album {
  font-size: 11px;
  font-weight: 400;
  letter-spacing: 0.4px;
  color: rgba(255, 255, 255, 0.45);
  white-space: nowrap;
  margin-top: 1px;
}
</style>