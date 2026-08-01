<script setup>
/**
 * ArtistAlbums — 歌手详情页专辑列表 Section
 *
 * 职责：
 *  - 渲染 section 标题 + AlbumList
 *  - section 标题入场动画（fadeInUp）
 *  - 空数据时不渲染（v-if 由父级控制，此处仅处理有数据场景）
 */
import { useTemplateRef, onMounted, watch, nextTick } from 'vue';
import AlbumList from '@/components/common/AlbumList.vue';
import { useAnime } from '@/composables/useAnime.js';

const props = defineProps({
  albums: { type: Array, default: () => [] },
});

const rootRef = useTemplateRef('root');
const { run } = useAnime(() => rootRef.value);

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
// albums 异步到达后触发
watch(() => props.albums, () => nextTick(playEnter), { flush: 'post' });
</script>

<template>
  <section ref="root" class="section">
    <div class="section-header">
      <h2 class="section-title">专辑</h2>
    </div>
    <AlbumList :albums="albums" />
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
