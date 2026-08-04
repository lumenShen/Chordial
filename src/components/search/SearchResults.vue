<script setup>
import TrackList from '@/components/common/TrackList.vue';
import ArtistList from '@/components/common/ArtistList.vue';
import AlbumList from '@/components/common/AlbumList.vue';

defineProps({
  songs: { type: Array, default: () => [] },
  artists: { type: Array, default: () => [] },
  albums: { type: Array, default: () => [] },
});
</script>

<template>
  <!-- 歌曲 -->
  <section v-if="songs.length > 0" class="result-section">
    <h2 class="section-title">
      歌曲 <span class="count-badge">{{ songs.length }}</span>
    </h2>
    <TrackList :tracks="songs" :show-match-count="true" mode="page" />
  </section>

  <!-- 歌手 -->
  <section v-if="artists.length > 0" class="result-section">
    <h2 class="section-title">
      歌手 <span class="count-badge">{{ artists.length }}</span>
    </h2>
    <ArtistList :artists="artists" :virtual-scroll="false" />
  </section>

  <!-- 专辑 -->
  <section v-if="albums.length > 0" class="result-section">
    <h2 class="section-title">
      专辑 <span class="count-badge">{{ albums.length }}</span>
    </h2>
    <AlbumList :albums="albums" />
  </section>
</template>

<style scoped>
/* ── 结果区 ── */
.result-section {
  margin-top: 32px;
}

.section-title {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 18px;
  font-weight: 700;
  color: var(--text-primary);
  margin: 0 0 16px 0;
}

.count-badge {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 24px;
  height: 22px;
  padding: 0 8px;
  border-radius: 11px;
  background: var(--bg-hover);
  color: var(--text-secondary);
  font-size: 12px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}
</style>