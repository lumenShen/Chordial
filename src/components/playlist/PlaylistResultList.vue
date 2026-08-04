<script setup>
/**
 * PlaylistResultList — 搜索结果的歌单卡片网格
 */
import PlaylistCard from './PlaylistCard.vue';

defineProps({
  results: { type: Array, default: () => [] },
  savedIds: { type: Set, default: () => new Set() },
  formatPlayCount: { type: Function, default: null },
});

const emit = defineEmits(['preview']);
</script>

<template>
  <div class="results-grid">
    <PlaylistCard
      v-for="item in results"
      :key="item.id"
      :item="item"
      :saved="savedIds.has(item.id)"
      :format-play-count="formatPlayCount"
      @click="emit('preview', $event)"
    />
  </div>
</template>

<style scoped>
.results-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  gap: 20px;
}

@media (max-width: 480px) {
  .results-grid {
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 12px;
  }
}
</style>