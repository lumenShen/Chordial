<script setup>
import { useMusicSourceManager } from '@/composables/useMusicSourceManager';
import PageHeader from '@/components/music-source/PageHeader.vue';
import LocalSourceList from '@/components/music-source/LocalSourceList.vue';
import OnlineSourceSection from '@/components/music-source/OnlineSourceSection.vue';
import UnsupportedNote from '@/components/music-source/UnsupportedNote.vue';

const {
  rootRef,
  folders,
  stats,
  isLoading,
  isScanning,
  isAdding,
  isRemoving,
  neteaseState,
  kugouState,
  getSourceTypeLabel,
  formatFileCount,
  handleScanAll,
  handleAddSource,
  handleDeleteSource,
  goToNeteaseSettings,
  goToKugouSettings,
} = useMusicSourceManager();
</script>

<template>
  <div ref="rootRef" class="music-source-page page-container">
    <!-- Header -->
    <PageHeader
      :is-loading="isLoading"
      :stats="stats"
      :is-scanning="isScanning"
      :is-adding="isAdding"
      :format-file-count="formatFileCount"
      @scan-all="handleScanAll"
      @add-source="handleAddSource"
    />

    <!-- 本地文件夹列表 + 加载 / 空状态 -->
    <LocalSourceList
      :is-loading="isLoading"
      :folders="folders"
      :is-removing="isRemoving"
      :get-source-type-label="getSourceTypeLabel"
      @delete-source="handleDeleteSource"
      @add-source="handleAddSource"
    />

    <!-- 在线来源：网易云 / 酷狗 -->
    <OnlineSourceSection
      :netease-state="neteaseState"
      :kugou-state="kugouState"
      @go-netease="goToNeteaseSettings"
      @go-kugou="goToKugouSettings"
    />

    <!-- Unsupported source notice -->
    <UnsupportedNote />
  </div>
</template>

<style scoped>
.music-source-page {
  /* max-width 和 margin 由 .page-container 提供 */
}
</style>