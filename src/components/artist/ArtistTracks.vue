<script setup>
/**
 * ArtistTracks — 歌手详情页热门歌曲 Section
 *
 * 职责：
 *  - 渲染 section 标题 + TrackList（前 N 首热门歌曲）
 *  - section 标题入场动画（fadeInUp）
 *  - TrackList 内部已接入 PlayerStore.play(track, tracks)，无需在此重复
 *
 * 注意：
 *  - mode=page 使用外层 .content-wrapper 滚动，避免自带 max-height 滚动容器造成多层滚动条
 *  - :virtual-scroll=false 仅渲染前 N 首，无需虚拟列表，避免独立滚动容器
 */
import { computed, useTemplateRef, onMounted, watch, nextTick } from 'vue';
import TrackList from '@/components/common/TrackList.vue';
import { useAnime } from '@/composables/useAnime.js';

const props = defineProps({
  tracks: { type: Array, default: () => [] },
  // 展示前 N 首（默认 10）
  limit: { type: Number, default: 10 },
});

const emit = defineEmits(['select']);

const rootRef = useTemplateRef('root');
const { run } = useAnime(() => rootRef.value);

// 截取前 N 首，避免一次性渲染全部歌曲
const displayTracks = computed(() => {
  if (!props.tracks?.length) return [];
  return props.limit > 0 ? props.tracks.slice(0, props.limit) : props.tracks;
});

function playEnter() {
  run(({ animate, presets }) => {
    const root = rootRef.value;
    if (!root) return;
    animate('.section-header', {
      ...presets.fadeInUp,
      onComplete: () => {
        rootRef.value?.querySelector('.section-header')?.style.removeProperty('transform');
      },
    });
  });
}

onMounted(playEnter);
watch(() => props.tracks, () => nextTick(playEnter), { flush: 'post' });

const handleSelect = (track) => emit('select', track);
const handlePlay = () => {
  /* TrackList 内部已调用 PlayerStore.play，此处仅为事件转发占位 */
};
</script>

<template>
  <section ref="root" class="section">
    <div class="section-header">
      <h2 class="section-title">热门歌曲</h2>
    </div>
    <TrackList
      :tracks="displayTracks"
      mode="page"
      :virtual-scroll="false"
      @select="handleSelect"
      @play="handlePlay"
    />
  </section>
</template>

<style scoped>
.section {
  margin-bottom: var(--space-8);
}

.section-header {
  margin-bottom: var(--space-5);
}

.section-title {
  font-size: var(--font-size-lg);
  font-weight: 700;
  color: var(--text-primary);
  margin: 0;
  letter-spacing: -0.3px;
}

@media (max-width: 767px) {
  .section {
    margin-bottom: var(--space-6);
  }

  .section-title {
    font-size: 18px;
  }
}
</style>
