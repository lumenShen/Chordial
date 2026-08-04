<script setup>
import { nextTick, watch, useTemplateRef } from 'vue';
import TrackList from '../components/common/TrackList.vue';
import ArtistList from '../components/common/ArtistList.vue';
import HomeHero from '../components/home/HomeHero.vue';
import HomeTrackCard from '../components/home/HomeTrackCard.vue';
import HomeAlbumCard from '../components/home/HomeAlbumCard.vue';
import HomeSection from '../components/home/HomeSection.vue';
import HomeEmptyState from '../components/home/HomeEmptyState.vue';
import { useHomeData } from '../composables/useHomeData.js';
import { useAnime } from '@/composables/useAnime.js';

const {
  recentTracks,
  featuredArtists,
  recentAlbums,
  stats,
  isLoading,
  greeting,
  todayPicks,
  featuredAlbums,
  handleTrackSelect,
  handleAlbumSelect,
  handleTrackPlay,
  playAllTracks,
  shufflePlay,
} = useHomeData();

// anime.js 作用域限定到组件根节点，卸载时自动 revert
const rootRef = useTemplateRef('root');
const { run } = useAnime(() => rootRef.value);

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
</script>

<template>
  <div class="home-page page-container" ref="root">
    <!-- Hero Banner -->
    <HomeHero
      :albums="recentAlbums"
      :visible="todayPicks.length > 0"
      :greeting="greeting"
      :stats="stats"
      @play-all="playAllTracks"
      @shuffle="shufflePlay"
    />

    <div v-if="isLoading" class="loading-state">
      <div class="spinner"></div>
      <span>加载中...</span>
    </div>

    <template v-else>
      <!-- 为你推荐 - 横向滚动 -->
      <HomeSection v-if="todayPicks.length > 0" title="为你推荐" to="/tracks">
        <div class="horizontal-scroll">
          <HomeTrackCard
            v-for="track in todayPicks"
            :key="track.id"
            :track="track"
            @select="handleTrackSelect"
            @play="handleTrackPlay"
          />
        </div>
      </HomeSection>

      <!-- 热门专辑 - 网格布局 -->
      <HomeSection v-if="featuredAlbums.length > 0" title="热门专辑" to="/albums">
        <div class="albums-grid">
          <HomeAlbumCard
            v-for="album in featuredAlbums"
            :key="album.id"
            :album="album"
            @select="handleAlbumSelect"
          />
        </div>
      </HomeSection>

      <!-- 最近添加 - 列表 -->
      <HomeSection v-if="recentTracks.length > 0" title="最近添加" to="/tracks" compact>
        <TrackList
          :tracks="recentTracks.slice(0, 5)"
          @select="handleTrackSelect"
          @play="handleTrackPlay"
        />
      </HomeSection>

      <!-- 推荐歌手 -->
      <HomeSection v-if="featuredArtists.length > 0" title="推荐歌手" to="/artists">
        <ArtistList :artists="featuredArtists.slice(0, 5)" />
      </HomeSection>

      <!-- 空状态 -->
      <HomeEmptyState
        v-if="recentTracks.length === 0 && featuredArtists.length === 0 && recentAlbums.length === 0"
      />
    </template>
  </div>
</template>

<style scoped>
.home-page {
  /* max-width 和 margin 由 .page-container 提供 */
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

/* 专辑网格 */
.albums-grid {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 24px;
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

/* 响应式 */
@media (max-width: 1024px) {
  .albums-grid {
    grid-template-columns: repeat(3, 1fr);
  }
}

@media (max-width: 767px) {
  .albums-grid {
    grid-template-columns: repeat(2, 1fr);
    gap: 16px;
  }
}

@media (max-width: 480px) {
  .albums-grid {
    grid-template-columns: repeat(2, 1fr);
    gap: 12px;
  }
}
</style>