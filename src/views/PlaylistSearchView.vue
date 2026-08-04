<script setup>
/**
 * PlaylistSearchView — 在线搜索网易云歌单并保存到本地库
 *
 * 薄壳：仅负责页面头部与状态区/结果列表/预览弹窗/Toast 的编排，
 * 业务逻辑集中在 usePlaylistSearch，UI 拆分到 components/playlist/。
 */
import { usePlaylistSearch } from '@/composables/usePlaylistSearch.js';
import PlaylistSearchBar from '@/components/playlist/PlaylistSearchBar.vue';
import PlaylistStatus from '@/components/playlist/PlaylistStatus.vue';
import PlaylistResultList from '@/components/playlist/PlaylistResultList.vue';
import PlaylistPreviewDialog from '@/components/playlist/PlaylistPreviewDialog.vue';
import PlaylistToast from '@/components/playlist/PlaylistToast.vue';

const {
  query,
  limit,
  results,
  isLoading,
  errorMsg,
  previewDetail,
  previewLoading,
  previewTracks,
  saving,
  savedIds,
  toast,
  hasResults,
  onInput,
  onSubmit,
  clearSearch,
  executeSearch,
  previewPlaylist,
  closePreview,
  saveToLocal,
  goLocalPlaylists,
  formatPlayCount,
} = usePlaylistSearch();
</script>

<template>
  <div class="playlist-search page-container">
    <div class="page-header">
      <div class="header-with-back">
        <button class="back-btn" @click="goLocalPlaylists">
          <HIcon name="arrow-left" />
        </button>
        <div>
          <h1 class="page-title">搜索网易云歌单</h1>
          <p class="page-subtitle">搜索在线歌单并保存到本地音乐库</p>
        </div>
      </div>
    </div>

    <!-- 搜索框 -->
    <PlaylistSearchBar
      v-model:query="query"
      v-model:limit="limit"
      @input="onInput"
      @submit="onSubmit"
      @clear="clearSearch"
      @limit-change="executeSearch"
    />

    <!-- 状态区：错误 / 加载 / 空态 -->
    <template v-if="errorMsg || isLoading || !hasResults">
      <PlaylistStatus
        :error-msg="errorMsg"
        :is-loading="isLoading"
        :query="query"
        :has-results="hasResults"
      />
    </template>

    <!-- 结果列表 -->
    <PlaylistResultList
      v-else
      :results="results"
      :saved-ids="savedIds"
      :format-play-count="formatPlayCount"
      @preview="previewPlaylist"
    />

    <!-- 预览弹窗 -->
    <PlaylistPreviewDialog
      :detail="previewDetail"
      :tracks="previewTracks"
      :loading="previewLoading"
      :saving="saving"
      @close="closePreview"
      @save="saveToLocal"
    />

    <!-- Toast -->
    <PlaylistToast :toast="toast" />
  </div>
</template>

<style scoped>
.playlist-search {
  /* max-width 和 margin 由 .page-container 提供 */
}

/* page-header 使用全局统一样式 */

.back-btn {
  width: 36px;
  height: 36px;
  border: 1px solid var(--border-light, #e8e8e8);
  background: var(--bg-secondary, #fff);
  border-radius: 50%;
  color: var(--text-secondary, #666);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: all 0.2s ease;
}

.back-btn:hover {
  border-color: var(--primary-color);
  color: var(--primary-color);
}

/* page-title / page-subtitle 使用全局统一样式（28px/800） */

.header-with-back {
  display: flex;
  align-items: center;
  gap: 12px;
  flex: 1;
  min-width: 0;
}
</style>