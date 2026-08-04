<script setup>
/**
 * ArtistDetail — 歌手详情页主视图
 *
 * 架构（减少上帝文件）：
 *   useArtistDetail  数据加载（取消令牌 + 路由 param 响应）
 *   ArtistHero       顶部封面 + 信息 + 操作按钮
 *   ArtistAlbums     专辑列表 section
 *   ArtistTracks     热门歌曲 section
 *   本视图仅做三态编排（loading / error / content）与动画管线
 *
 * 动画全流程管线：
 *  - 进入：routeEnter（路由层）→ loading spinner（ANIME_LOOP.spin）
 *  - loading→content：spinner 淡出 + content 区 fadeInUp
 *  - 切换歌手（路由 param 变化）：useArtistDetail 取消令牌防抖 +
 *    :key="artist.id" 重建 content 区 → ArtistHero/Albums/Tracks 各自重播入场
 *  - error：content 淡出 → empty-state scaleIn
 */
import { ref, computed, useTemplateRef, watch, nextTick, onMounted } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import GradualBlur from '@/components/ui/GradualBlur.vue';
import ArtistHero from '@/components/artist/ArtistHero.vue';
import ArtistAlbums from '@/components/artist/ArtistAlbums.vue';
import ArtistTracks from '@/components/artist/ArtistTracks.vue';
import { useArtistDetail } from '@/composables/useArtistDetail.js';
import { useAnime } from '@/composables/useAnime.js';
import PlayerStore from '@/stores/player/index.js';

const route = useRoute();
const router = useRouter();

// 路由 param 作为响应式源，watch 触发重新加载（支持同组件复用切换歌手）
const artistIdRef = computed(() => route.params.artistId);
const { artist, albums, tracks, isLoading, error, reload } = useArtistDetail(artistIdRef);

const rootRef = useTemplateRef('root');
const { run } = useAnime(() => rootRef.value);

// 收藏状态（TODO: 接入收藏 store，当前仅 UI 状态）
const isFavorite = ref(false);

// --- 动画 ---
// loading spinner 旋转：用 ANIME_LOOP.spin 替代 CSS @keyframes
function playLoadingSpinner() {
  run(({ animate, loopPresets }) => {
    animate('.loading-state .spinner', { ...loopPresets.spin });
  });
}

// 三态切换时编排：
//  - 进入 loading：启动 spinner
//  - 进入 content：content 区整体 fadeInUp（子组件内部各自播 cover/title 动画）
//  - 进入 empty：scaleIn
watch(
  isLoading,
  (val) => {
    if (val) {
      nextTick(playLoadingSpinner);
    }
  },
  { flush: 'post' },
);

// 初始挂载若处于 loading，启动 spinner
onMounted(() => {
  if (isLoading.value) {
    nextTick(playLoadingSpinner);
  }
});

// 播放全部：将歌手全部歌曲注入播放列表并播放第一首
function handlePlayAll() {
  if (!tracks.value?.length) return;
  PlayerStore.play(tracks.value[0], tracks.value);
}

function handleToggleFavorite() {
  isFavorite.value = !isFavorite.value;
  // TODO: 接入收藏 store 持久化
}

// 歌曲点击 → 跳转歌曲详情
function handleTrackSelect(track) {
  router.push(`/track/${track.id}`);
}

// 无歌手数据且非加载中 → 显示空状态
const showEmpty = computed(() => !artist.value && !isLoading.value && !error.value);
const showError = computed(() => !!error.value && !isLoading.value);
</script>

<template>
  <div ref="root" class="artist-detail-page page-container">
    <!-- 顶部渐进模糊：内容从 header 下方渐显，消除滚动时 overflow 硬切割 -->
    <GradualBlur
      target="page"
      position="top"
      height="5rem"
      :z-index="-50"
      :strength="2"
      :div-count="4"
      :style="{ top: 'calc(var(--header-height) + var(--safe-area-top))' }"
    />
    <!-- 底部渐进模糊：内容渐隐到底栏上方 -->
    <GradualBlur
      target="page"
      position="bottom"
      height="5rem"
      :z-index="-50"
      :strength="2"
      :div-count="4"
      :style="{ bottom: 'calc(var(--bottom-nav-height) + var(--player-bar-height) + var(--safe-area-bottom))' }"
    />

    <!-- loading 态 -->
    <div v-if="isLoading" class="loading-state">
      <div class="spinner"></div>
    </div>

    <!-- 错误态 -->
    <div v-else-if="showError" class="empty-state">
      <svg class="empty-state-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <circle cx="12" cy="12" r="10" />
        <line x1="12" y1="8" x2="12" y2="12" />
        <line x1="12" y1="16" x2="12.01" y2="16" />
      </svg>
      <h3 class="empty-state-title">加载失败</h3>
      <p class="empty-state-desc">{{ error.message || '无法获取该歌手的信息' }}</p>
      <button class="btn btn-primary" style="margin-top: 16px;" @click="reload(artistIdRef.value)">
        重试
      </button>
    </div>

    <!-- 内容态：key=artist.id 切换歌手时重建，触发子组件入场动画 -->
    <div
      v-else-if="artist"
      :key="artist.id"
      class="artist-content"
    >
      <ArtistHero
        :artist="artist"
        :is-favorite="isFavorite"
        @play-all="handlePlayAll"
        @toggle-favorite="handleToggleFavorite"
      />

      <ArtistAlbums v-if="albums.length > 0" :albums="albums" />

      <ArtistTracks
        v-if="tracks.length > 0"
        :tracks="tracks"
        :limit="10"
        @select="handleTrackSelect"
      />
    </div>

    <!-- 空状态 -->
    <div v-else-if="showEmpty" class="empty-state">
      <svg class="empty-state-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <circle cx="12" cy="12" r="10" />
        <line x1="12" y1="8" x2="12" y2="12" />
        <line x1="12" y1="16" x2="12.01" y2="16" />
      </svg>
      <h3 class="empty-state-title">歌手不存在</h3>
      <p class="empty-state-desc">找不到该歌手的信息</p>
      <router-link to="/artists" class="btn btn-primary" style="margin-top: 16px;">
        返回歌手列表
      </router-link>
    </div>
  </div>
</template>

<style scoped>
.artist-detail-page {
  position: relative;
}

.artist-content {
  /* 内容区入场由子组件各自处理，此处仅做容器 */
}

/* 覆盖全局 .spinner 的 CSS 旋转，改由 anime.js 驱动 */
.spinner {
  animation: none;
}

@media (prefers-color-scheme: dark) {
  /* 暗色模式由 tokens.css 变量自动切换 */
}
</style>
