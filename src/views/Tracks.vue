<script setup>
import { onMounted, watch, nextTick, useTemplateRef } from 'vue';
import { useRouter } from 'vue-router';
import TrackList from '../components/common/TrackList.vue';
import TracksEmptyState from '../components/common/TracksEmptyState.vue';
import { useAnime } from '@/composables/useAnime.js';
import { useTracksPagination } from '@/composables/useTracksPagination.js';

const router = useRouter();

const rootRef = useTemplateRef('root');
const { run } = useAnime(() => rootRef.value);

const {
  tracks,
  totalCount,
  isLoading,
  hasMore,
  loadTracks,
  loadMore,
  playLoadingSpinner,
} = useTracksPagination({ run });

// 页面挂载时获取数据（已加载则跳过）
onMounted(() => {
  if (isLoading.value) {
    nextTick(playLoadingSpinner);
  }
  if (tracks.value.length === 0) {
    loadTracks();
  } else {
    isLoading.value = false;
  }
});

// loading 状态切换：启动 spinner（列表入场由 TrackList 组件内部 playEnter 处理）
watch(
  isLoading,
  (val) => {
    if (val) {
      nextTick(playLoadingSpinner);
    }
  },
  { flush: 'post' }
);

const handleTrackSelect = (track) => {
  router.push(`/track/${track.id}`);
};

const handleTrackPlay = (track) => {
  // TODO: 接入播放逻辑
};
</script>

<template>
  <div class="tracks-page page-container" ref="root">
    <div class="page-header">
      <div>
        <h1 class="page-title">歌曲</h1>
        <p class="page-subtitle">共 {{ totalCount || tracks.length }} 首歌曲</p>
      </div>
    </div>

    <div v-if="isLoading" class="loading-state">
      <div class="spinner"></div>
    </div>

    <template v-else>
      <TrackList
        v-if="tracks.length > 0"
        :tracks="tracks"
        mode="page"
        :infinite-scroll="true"
        :on-load-more="loadMore"
        :has-more="hasMore"
        :show-match-count="true"
        @select="handleTrackSelect"
        @play="handleTrackPlay"
      />

      <TracksEmptyState v-else-if="tracks.length === 0" />
    </template>
  </div>
</template>

<style scoped>
/* 禁用全局 .spinner 的 CSS spin 动画，改由 anime.js ANIME_LOOP.spin 驱动 */
.loading-state .spinner {
  animation: none;
}
</style>