<script setup>
import { ref, shallowRef, onMounted } from 'vue';
import { useRouter } from 'vue-router';
import { HugeiconsIcon } from '@hugeicons/vue';
import { HeartIcon, PlayIcon, ShuffleIcon } from '@hugeicons/core-free-icons';
import TrackList from '@/components/common/TrackList.vue';
import PlayerStore from '@/stores/player/index.js';
import { getFavoriteTracks, useFavorites } from '@/composables/useFavorites.js';

const router = useRouter();
const { load } = useFavorites();

const tracks = shallowRef([]);
const isLoading = ref(true);

async function loadFavorites() {
  isLoading.value = true;
  await load();
  tracks.value = await getFavoriteTracks();
  isLoading.value = false;
}

onMounted(loadFavorites);

const playAll = () => {
  if (!tracks.value.length) return;
  PlayerStore.play(tracks.value[0], tracks.value);
};

const shufflePlay = () => {
  if (!tracks.value.length) return;
  const shuffled = [...tracks.value].sort(() => 0.5 - Math.random());
  PlayerStore.play(shuffled[0], shuffled);
};

const handleTrackSelect = (track) => {
  router.push(`/track/${track.id}`);
};

const handleTrackPlay = (track) => {
  PlayerStore.play(track, tracks.value);
};
</script>

<template>
  <div class="favorites-page page-container">
    <div class="page-header">
      <div>
        <h1 class="page-title">
          <HugeiconsIcon :icon="HeartIcon" :size="24" class="title-heart" />
          偏好库
        </h1>
        <p class="page-subtitle">收藏的歌曲都在这里 — {{ tracks.length }} 首</p>
      </div>
      <div class="page-actions">
        <button class="btn btn-secondary" @click="shufflePlay" :disabled="!tracks.length">
          <HugeiconsIcon :icon="ShuffleIcon" :size="15" />
          随机播放
        </button>
        <button class="btn btn-primary" @click="playAll" :disabled="!tracks.length">
          <HugeiconsIcon :icon="PlayIcon" :size="15" />
          播放全部
        </button>
      </div>
    </div>

    <div v-if="isLoading" class="loading-state">
      <div class="spinner"></div>
    </div>

    <div v-else-if="tracks.length === 0" class="empty-state">
      <HugeiconsIcon :icon="HeartIcon" :size="40" class="empty-state-icon" />
      <h3 class="empty-state-title">偏好库还是空的</h3>
      <p class="empty-state-desc">在歌曲列表点击爱心即可收藏喜欢的歌曲</p>
    </div>

    <TrackList
      v-else
      :tracks="tracks"
      @select="handleTrackSelect"
      @play="handleTrackPlay"
    />
  </div>
</template>

<style scoped>
.title-heart {
  vertical-align: -4px;
  margin-right: 6px;
  color: #e0245e;
}

.page-actions .btn {
  display: flex;
  align-items: center;
  gap: 6px;
}

.empty-state-icon {
  color: #e0245e;
  margin-bottom: var(--space-4);
}
</style>
