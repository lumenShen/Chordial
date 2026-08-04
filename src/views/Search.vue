<script setup>
import { useTemplateRef } from 'vue';
import SearchHeader from '@/components/search/SearchHeader.vue';
import SearchStatus from '@/components/search/SearchStatus.vue';
import SearchResults from '@/components/search/SearchResults.vue';
import { useSearch } from '@/composables/useSearch.js';

// 页面根节点：供 useSearch 的 anime.js 作用域限定加载动画
const rootRef = useTemplateRef('root');

const {
  query,
  entityType,
  sourceName,
  limitPerType,
  songs,
  artists,
  albums,
  isLoading,
  hasSearched,
  errorMsg,
  searchTimeMs,
  neteaseWarning,
  totalCount,
  hasAnyResult,
  onInput,
  onSubmit,
  clearSearch,
} = useSearch(() => rootRef.value);
</script>

<template>
  <div class="search-page page-container" ref="root">
    <SearchHeader
      v-model:query="query"
      v-model:entityType="entityType"
      v-model:sourceName="sourceName"
      v-model:limitPerType.number="limitPerType"
      :search-time-ms="searchTimeMs"
      :is-loading="isLoading"
      :total-count="totalCount"
      @input="onInput"
      @submit="onSubmit"
      @clear="clearSearch"
    />

    <!-- 结果区 -->
    <SearchResults
      v-if="!errorMsg && !isLoading && hasSearched && hasAnyResult"
      :songs="songs"
      :artists="artists"
      :albums="albums"
    />

    <!-- 状态区：错误 / 警告 / 加载 / 空态（内部 v-else-if 链保证优先级） -->
    <SearchStatus
      v-else
      :error-msg="errorMsg"
      :netease-warning="neteaseWarning"
      :is-loading="isLoading"
      :has-searched="hasSearched"
      :has-any-result="hasAnyResult"
      :query="query"
    />
  </div>
</template>

<style scoped>
.search-page {
  /* max-width 和 margin 由 .page-container 提供 */
}
</style>