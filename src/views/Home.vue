<script setup>
import { ref, shallowRef, onMounted, computed, watch, nextTick, useTemplateRef } from 'vue';
import { useRouter } from 'vue-router';
import TrackList from '../components/common/TrackList.vue';
import ArtistList from '../components/common/ArtistList.vue';
import CoverImage from '../components/common/CoverImage.vue';
import AlbumCollageBackground from '../components/common/AlbumCollageBackground.vue';
import { library } from '../api/musicSource';
import PlayerStore from '@/stores/player/index.js';
import { usePerf } from '@/utils/performanceMonitor.js';
import { useAnime } from '@/composables/useAnime.js';

const { start, end, log } = usePerf('Home');

const router = useRouter();

// anime.js 作用域限定到组件根节点，卸载时自动 revert
const rootRef = useTemplateRef('root');
const { run } = useAnime(() => rootRef.value);

// shallowRef：列表数据为业务类实例，写入后只读，避免深代理开销
const recentTracks = shallowRef([]);
const featuredArtists = shallowRef([]);
const recentAlbums = shallowRef([]);
const stats = ref({
  artists: 0,
  albums: 0,
  tracks: 0
});
const isLoading = ref(true);

// 获取当前时间问候语
const greeting = computed(() => {
  const hour = new Date().getHours();
  if (hour < 6) return '夜深了';
  if (hour < 12) return '早上好';
  if (hour < 18) return '下午好';
  return '晚上好';
});

// 获取今日推荐歌曲（随机选择）
const todayPicks = computed(() => {
  if (recentTracks.value.length === 0) return [];
  const shuffled = [...recentTracks.value].sort(() => 0.5 - Math.random());
  return shuffled.slice(0, 5);
});

// 获取热门专辑（随机选择）
const featuredAlbums = computed(() => {
  if (recentAlbums.value.length === 0) return [];
  const shuffled = [...recentAlbums.value].sort(() => 0.5 - Math.random());
  return shuffled.slice(0, 4);
});

const loadHomeData = async () => {
  isLoading.value = true;
  start('loadHomeData');
  try {
    const data = await library.homeStats();
    if (data) {
      recentTracks.value = data.recentTracks;
      featuredArtists.value = data.featuredArtists;
      recentAlbums.value = data.recentAlbums;
      stats.value = data.stats;
    }
    end('loadHomeData', { tracks: recentTracks.value.length, artists: featuredArtists.value.length, albums: recentAlbums.value.length });
  } catch (error) {
    console.error('Failed to load home data:', error);
    end('loadHomeData', { error: error.message });
  } finally {
    isLoading.value = false;
  }
};

onMounted(() => {
  if (recentTracks.value.length === 0) {
    loadHomeData();
  } else {
    isLoading.value = false;
  }
});

// ===== anime.js 动画 =====
// 加载 spinner：用 ANIME_LOOP.spin 替代原 CSS @keyframes spin
const startSpinner = () => {
  run(({ animate, loopPresets }) => {
    animate('.spinner', { ...loopPresets.spin });
  });
};

// 监听加载状态：加载中旋转 spinner
// immediate 保证首次进入（isLoading 初始为 true）也能启动 spinner
watch(
  isLoading,
  (loading) => {
    if (loading) {
      nextTick(startSpinner);
    }
  },
  { immediate: true }
);

const handleTrackSelect = (track) => {
  router.push(`/track/${track.id}`);
};

const handleTrackPlay = (track) => {
  log('handleTrackPlay', { trackId: track.id, title: track.title });
  PlayerStore.play(track, recentTracks.value);
};

const playAllTracks = () => {
  if (recentTracks.value.length > 0) {
    log('playAllTracks', { count: recentTracks.value.length });
    PlayerStore.play(recentTracks.value[0], recentTracks.value);
  }
};

const shufflePlay = () => {
  if (recentTracks.value.length > 0) {
    log('shufflePlay', { count: recentTracks.value.length });
    const shuffled = [...recentTracks.value].sort(() => 0.5 - Math.random());
    PlayerStore.play(shuffled[0], shuffled);
  }
};


</script>

<template>
  <div class="home-page page-container" ref="root">
    <!-- Hero Banner -->
    <section class="hero-banner" v-if="todayPicks.length > 0">
      <!-- 多专辑封面拼接背景 -->
      <AlbumCollageBackground 
        :albums="recentAlbums" 
        :max-display="9"
        :animation-enabled="true"
        class="hero-collage-bg"
      />
      <div class="hero-content">
        <div class="hero-badge">
          <HIcon name="stars" />
          <span>今日推荐</span>
        </div>
        <h1 class="hero-title">{{ greeting }}，音乐爱好者</h1>
        <p class="hero-subtitle">探索你的音乐世界，发现无限可能</p>
        <div class="hero-actions">
          <button class="hero-btn primary" @click="playAllTracks">
            <HIcon name="play-fill" />
            <span>播放全部</span>
          </button>
          <button class="hero-btn secondary" @click="shufflePlay">
            <HIcon name="shuffle" />
            <span>随机播放</span>
          </button>
        </div>
      </div>
      <div class="hero-stats">
        <div class="hero-stat">
          <span class="hero-stat-value">{{ stats.tracks }}</span>
          <span class="hero-stat-label">首歌曲</span>
        </div>
        <div class="hero-stat-divider"></div>
        <div class="hero-stat">
          <span class="hero-stat-value">{{ stats.albums }}</span>
          <span class="hero-stat-label">张专辑</span>
        </div>
        <div class="hero-stat-divider"></div>
        <div class="hero-stat">
          <span class="hero-stat-value">{{ stats.artists }}</span>
          <span class="hero-stat-label">位歌手</span>
        </div>
      </div>
    </section>

    <div v-if="isLoading" class="loading-state">
      <div class="spinner"></div>
      <span>加载中...</span>
    </div>

    <template v-else>
      <!-- 为你推荐 - 横向滚动 -->
      <section class="section" v-if="todayPicks.length > 0">
        <div class="section-header">
          <h2 class="section-title">为你推荐</h2>
          <router-link to="/tracks" class="section-link">
            查看全部
            <HIcon name="arrow-right" />
          </router-link>
        </div>
        <div class="horizontal-scroll">
          <div 
            v-for="track in todayPicks" 
            :key="track.id"
            class="track-card"
            @click="handleTrackSelect(track)"
          >
            <div class="track-card-cover">
              <CoverImage
                :item="track"
                :alt="track.title"
                type="track"
                priority
              />
              <button class="track-card-play" @click.stop="handleTrackPlay(track)">
                <HIcon name="play-fill" />
              </button>
            </div>
            <div class="track-card-info">
              <h3 class="track-card-title">{{ track.title || track.fileName || '未知歌曲' }}</h3>
              <p class="track-card-artist">{{ track.getDisplayArtist ? track.getDisplayArtist() : (track.artist || '未知歌手') }}</p>
            </div>
          </div>
        </div>
      </section>

      <!-- 热门专辑 - 网格布局 -->
      <section class="section" v-if="featuredAlbums.length > 0">
        <div class="section-header">
          <h2 class="section-title">热门专辑</h2>
          <router-link to="/albums" class="section-link">
            查看全部
            <HIcon name="arrow-right" />
          </router-link>
        </div>
        <div class="albums-grid">
          <div 
            v-for="album in featuredAlbums" 
            :key="album.id"
            class="album-card"
            @click="router.push(`/albums/${album.id}`)"
          >
            <div class="album-cover-wrapper">
              <CoverImage
                :item="album"
                :alt="album.title"
                type="album"
                priority
              />
              <div class="album-overlay">
                <button class="album-play-btn">
                  <HIcon name="play-fill" />
                </button>
              </div>
            </div>
            <div class="album-info">
              <h3 class="album-title">{{ album.title || '未知专辑' }}</h3>
              <p class="album-artist">{{ album.artist || '未知歌手' }}</p>
            </div>
          </div>
        </div>
      </section>

      <!-- 最近添加 - 列表 -->
      <section class="section section-compact" v-if="recentTracks.length > 0">
        <div class="section-header">
          <h2 class="section-title">最近添加</h2>
          <router-link to="/tracks" class="section-link">
            查看全部
            <HIcon name="arrow-right" />
          </router-link>
        </div>
        <TrackList 
          :tracks="recentTracks.slice(0, 5)" 
          @select="handleTrackSelect"
          @play="handleTrackPlay"
        />
      </section>

      <!-- 推荐歌手 -->
      <section class="section" v-if="featuredArtists.length > 0">
        <div class="section-header">
          <h2 class="section-title">推荐歌手</h2>
          <router-link to="/artists" class="section-link">
            查看全部
            <HIcon name="arrow-right" />
          </router-link>
        </div>
        <ArtistList :artists="featuredArtists.slice(0, 5)" />
      </section>

      <!-- 空状态 -->
      <div v-if="recentTracks.length === 0 && featuredArtists.length === 0 && recentAlbums.length === 0" class="empty-state">
        <div class="empty-icon">
          <HIcon name="music-note-beamed" />
        </div>
        <h3 class="empty-title">开始你的音乐之旅</h3>
        <p class="empty-desc">添加音乐源，发现属于你的音乐世界</p>
        <router-link to="/music-sources" class="empty-action">
          <HIcon name="plus-lg" />
          添加音乐源
        </router-link>
      </div>
    </template>
  </div>
</template>

<style scoped>
.home-page {
  /* max-width 和 margin 由 .page-container 提供 */
}

/* Hero Banner - 白/灰/黑三色调沉浸式头部 */
.hero-banner {
  position: relative;
  margin-bottom: 40px;
  padding: 40px 44px;
  border-radius: var(--radius-xl);
  overflow: hidden;
  min-height: 300px;
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  background: linear-gradient(135deg, #ffffff 0%, #f4f4f5 55%, #ececef 100%);
  border: 1px solid var(--border-light);
}

/* 多专辑封面背景 */
.hero-collage-bg {
  position: absolute;
  inset: 0;
  z-index: 0;
  opacity: 0.28;
}

.hero-content {
  position: relative;
  z-index: 1;
  max-width: 520px;
}

.hero-badge {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 5px 12px;
  background: var(--bg-tertiary);
  border: 1px solid var(--border-color);
  backdrop-filter: blur(8px);
  border-radius: 20px;
  color: var(--text-primary);
  font-size: 12px;
  font-weight: 600;
  margin-bottom: 16px;
  will-change: backdrop-filter;
  contain: paint;
}

.hero-badge i {
  color: var(--text-primary);
}

.hero-title {
  font-size: 38px;
  font-weight: 800;
  color: var(--text-primary);
  margin: 0 0 12px 0;
  letter-spacing: -1px;
  line-height: 1.1;
}

.hero-subtitle {
  font-size: 15px;
  color: var(--text-secondary);
  margin: 0 0 24px 0;
  font-weight: 400;
}

.hero-actions {
  display: flex;
  gap: 12px;
}

.hero-btn {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px 24px;
  border-radius: 100px;
  font-size: 14px;
  font-weight: 600;
  cursor: pointer;
  transition: all var(--transition-fast);
  border: none;
}

.hero-btn.primary {
  background: var(--primary-color);
  color: var(--primary-contrast);
  box-shadow: var(--shadow-primary);
}

.hero-btn.primary:hover {
  transform: scale(1.04);
  box-shadow: var(--shadow-md);
}

.hero-btn.secondary {
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  color: var(--text-primary);
}

.hero-btn.secondary:hover {
  background: var(--bg-hover);
}

.hero-btn i {
  font-size: 16px;
}

.hero-stats {
  position: relative;
  z-index: 1;
  display: flex;
  align-items: center;
  gap: 20px;
  padding: 18px 22px;
  background: var(--bg-secondary);
  backdrop-filter: blur(12px);
  border-radius: var(--radius-lg);
  border: 1px solid var(--border-color);
  box-shadow: var(--shadow-sm);
  will-change: backdrop-filter;
  contain: layout paint;
}

.hero-stat {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
}

.hero-stat-value {
  font-size: 26px;
  font-weight: 800;
  color: var(--text-primary);
  line-height: 1;
  letter-spacing: -1px;
}

.hero-stat-label {
  font-size: 11px;
  color: var(--text-secondary);
  font-weight: 500;
}

.hero-stat-divider {
  width: 1px;
  height: 36px;
  background: var(--border-color);
}

/* 区块样式 */
.section {
  margin-bottom: 48px;
}

.section-compact {
  margin-bottom: 40px;
}

.section-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 20px;
}

.section-title {
  font-size: 24px;
  font-weight: 700;
  color: var(--text-primary);
  margin: 0;
  letter-spacing: -0.5px;
}

.section-link {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 14px;
  font-weight: 600;
  color: var(--primary-color);
  text-decoration: none;
  padding: 8px 16px;
  border-radius: 100px;
  transition: all var(--transition-fast);
  background: transparent;
}

.section-link:hover {
  background: var(--primary-light);
}

.section-link i {
  font-size: 14px;
}

/* 横向滚动 - 为你推荐 */
.horizontal-scroll {
  display: flex;
  gap: 20px;
  overflow-x: auto;
  overflow-y: visible;
  padding-bottom: 16px;
  margin-bottom: -8px;
  scrollbar-width: none;
  -ms-overflow-style: none;
}

.horizontal-scroll::-webkit-scrollbar {
  display: none;
}

.track-card {
  flex: 0 0 180px;
  cursor: pointer;
  transition: transform var(--transition-spring);
}

.track-card:hover {
  transform: translateY(-8px);
}

.track-card:hover .track-card-play {
  opacity: 1;
  transform: scale(1);
}

.track-card-cover {
  position: relative;
  aspect-ratio: 1;
  border-radius: var(--radius-lg);
  overflow: hidden;
  margin-bottom: 12px;
  box-shadow: var(--shadow-sm);
  transition: box-shadow var(--transition-normal);
}

.track-card:hover .track-card-cover {
  box-shadow: var(--shadow-md);
}

.track-card-cover :deep(.cover-wrapper) {
  width: 100%;
  height: 100%;
}

.track-card-play {
  position: absolute;
  bottom: 12px;
  right: 12px;
  width: 44px;
  height: 44px;
  border-radius: 50%;
  background: var(--primary-color);
  color: white;
  border: none;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  opacity: 0;
  transform: scale(0.8);
  transition: all var(--transition-fast);
  box-shadow: var(--shadow-primary);
}

.track-card-play:hover {
  transform: scale(1.1) !important;
}

.track-card-play i {
  font-size: 20px;
  margin-left: 2px;
}

.track-card-info {
  padding: 0 4px;
}

.track-card-title {
  font-size: 15px;
  font-weight: 600;
  color: var(--text-primary);
  margin: 0 0 4px 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  letter-spacing: -0.2px;
}

.track-card-artist {
  font-size: 13px;
  color: var(--text-secondary);
  margin: 0;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* 专辑网格 */
.albums-grid {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 24px;
}

.album-card {
  cursor: pointer;
  transition: transform var(--transition-spring);
}

.album-card:hover {
  transform: translateY(-8px);
}

.album-card:hover .album-overlay {
  opacity: 1;
}

.album-cover-wrapper {
  position: relative;
  aspect-ratio: 1;
  border-radius: var(--radius-lg);
  overflow: hidden;
  margin-bottom: 14px;
  box-shadow: var(--shadow-sm);
  transition: box-shadow var(--transition-normal);
}

.album-card:hover .album-cover-wrapper {
  box-shadow: var(--shadow-md);
}

.album-cover-wrapper :deep(.cover-wrapper) {
  width: 100%;
  height: 100%;
}

.album-overlay {
  position: absolute;
  inset: 0;
  background: rgba(0, 0, 0, 0.4);
  display: flex;
  align-items: center;
  justify-content: center;
  opacity: 0;
  transition: opacity var(--transition-fast);
}

.album-play-btn {
  width: 56px;
  height: 56px;
  border-radius: 50%;
  background: var(--primary-color);
  color: white;
  border: none;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  transition: transform var(--transition-fast);
  box-shadow: var(--shadow-primary);
}

.album-play-btn:hover {
  transform: scale(1.1);
}

.album-play-btn i {
  font-size: 24px;
  margin-left: 2px;
}

.album-info {
  padding: 0 4px;
}

.album-title {
  font-size: 15px;
  font-weight: 600;
  color: var(--text-primary);
  margin: 0 0 5px 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  letter-spacing: -0.2px;
}

.album-artist {
  font-size: 13px;
  color: var(--text-secondary);
  margin: 0;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* 加载状态 */
.loading-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 100px 0;
  gap: 16px;
  color: var(--text-secondary);
}

.spinner {
  width: 44px;
  height: 44px;
  border: 3px solid var(--border-light);
  border-top-color: var(--primary-color);
  border-radius: 50%;
}

/* 空状态 */
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 100px 40px;
  text-align: center;
}

.empty-icon {
  width: 80px;
  height: 80px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--primary-light);
  color: var(--primary-color);
  border-radius: 50%;
  font-size: 36px;
  margin-bottom: 24px;
}

.empty-title {
  font-size: 24px;
  font-weight: 700;
  color: var(--text-primary);
  margin: 0 0 8px 0;
}

.empty-desc {
  font-size: 15px;
  color: var(--text-secondary);
  margin: 0 0 28px 0;
}

.empty-action {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 14px 28px;
  background: var(--primary-color);
  color: white;
  border-radius: 100px;
  font-size: 15px;
  font-weight: 600;
  text-decoration: none;
  transition: all var(--transition-fast);
  box-shadow: var(--shadow-primary);
}

.empty-action:hover {
  background: var(--primary-hover);
  transform: translateY(-2px);
  box-shadow: 0 6px 20px rgba(0, 122, 255, 0.35);
}

/* 响应式 */
@media (max-width: 1024px) {
  .hero-banner {
    flex-direction: column;
    align-items: flex-start;
    gap: 32px;
    padding: 32px;
  }

  .hero-stats {
    width: 100%;
    justify-content: space-around;
  }

  .albums-grid {
    grid-template-columns: repeat(3, 1fr);
  }
}

@media (max-width: 767px) {
  .hero-banner {
    padding: 28px;
    min-height: 280px;
  }

  .hero-title {
    font-size: 32px;
  }

  .hero-subtitle {
    font-size: 14px;
  }

  .hero-btn {
    padding: 12px 20px;
    font-size: 14px;
  }

  .hero-stats {
    padding: 16px 20px;
    gap: 16px;
  }

  .hero-stat-value {
    font-size: 22px;
  }

  .section-title {
    font-size: 20px;
  }

  .track-card {
    flex: 0 0 150px;
  }

  .track-card-play {
    width: 44px;
    height: 44px;
    opacity: 1;
    transform: scale(1);
    bottom: 8px;
    right: 8px;
  }

  .track-card-play i {
    font-size: 16px;
  }

  .albums-grid {
    grid-template-columns: repeat(2, 1fr);
    gap: 16px;
  }

  .album-play-btn {
    width: 44px;
    height: 44px;
  }

  .album-play-btn i {
    font-size: 20px;
  }
}

@media (max-width: 480px) {
  .hero-actions {
    flex-wrap: wrap;
  }

  .hero-btn span {
    display: none;
  }

  .hero-btn i {
    margin: 0;
    font-size: 20px;
  }

  .albums-grid {
    grid-template-columns: repeat(2, 1fr);
    gap: 12px;
  }
}

@media (prefers-color-scheme: dark) {
  .hero-banner {
    background: linear-gradient(135deg, #1b1b1d 0%, #161618 55%, #111113 100%);
    border-color: var(--border-light);
  }

  .hero-badge {
    background: rgba(255, 255, 255, 0.08);
  }

  .hero-btn.secondary {
    background: rgba(255, 255, 255, 0.08);
  }

  .hero-stats {
    background: rgba(255, 255, 255, 0.05);
  }
}
</style>
