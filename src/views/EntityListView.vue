<script setup>
/**
 * EntityListView — 分页实体列表页骨架（歌手/专辑共用）
 *
 * 原 Artists.vue 与 Albums.vue 各 187 行、diff 仅标识符不同，收敛到此组件。
 * 使用方只需提供：取数函数、文案、列表渲染插槽。
 *
 * @example
 * <EntityListView title="歌手" unit="位歌手" :fetch-page="library.getArtistsPage" item-key="artists">
 *   <template #list="{ items }"><ArtistList :artists="items" /></template>
 * </EntityListView>
 */
import { ref, shallowRef, onMounted, watch, nextTick, useTemplateRef } from 'vue';
import { usePerf } from '@/utils/performanceMonitor.js';
import { useAnime } from '@/composables/useAnime.js';
import { useLibraryEvents } from '@/composables/useLibraryEvents.js';
import GradualBlur from '@/components/ui/GradualBlur.vue';

const props = defineProps({
  /** 页头标题，如 "歌手" */
  title: { type: String, required: true },
  /** 计数单位文案，如 "位歌手"（渲染为 "共 N 位歌手"） */
  unit: { type: String, required: true },
  /** 分页取数：(offset, pageSize) => Promise<{ total: number, [itemKey]: any[] }> */
  fetchPage: { type: Function, required: true },
  /** 返回数据中列表字段名，如 "artists" / "albums" */
  itemKey: { type: String, required: true },
  /** 空状态标题 */
  emptyTitle: { type: String, required: true },
  /** 空状态描述 */
  emptyDesc: { type: String, required: true },
  /** performanceMonitor 打点名称，默认取 title */
  perfName: { type: String, default: '' },
});

const { start, end } = usePerf(props.perfName || props.title);

const PAGE_SIZE = 50;
// shallowRef：列表数据为业务类实例，避免深代理开销
const items = shallowRef([]);
const totalCount = ref(0);
const isLoading = ref(true);
const isLoadingMore = ref(false);
const hasMore = ref(true);

const rootRef = useTemplateRef('root');
const { run } = useAnime(() => rootRef.value);

// 订阅全局库变更事件 — 移除/添加音乐源后自动刷新列表
const { libraryVersion } = useLibraryEvents();

const loadItems = async () => {
  isLoading.value = true;
  start('loadItems');
  try {
    const data = await props.fetchPage(0, PAGE_SIZE);
    if (data) {
      items.value = data[props.itemKey];
      totalCount.value = data.total;
      hasMore.value = data[props.itemKey].length < data.total;
    }
    end('loadItems', { count: items.value.length, total: totalCount.value });
  } catch (error) {
    console.error(`Failed to load ${props.title}:`, error);
    end('loadItems', { error: error.message });
  } finally {
    isLoading.value = false;
  }
};

const loadMore = async () => {
  if (isLoadingMore.value || !hasMore.value) return;
  isLoadingMore.value = true;
  try {
    const data = await props.fetchPage(items.value.length, PAGE_SIZE);
    if (data) {
      items.value = [...items.value, ...data[props.itemKey]];
      hasMore.value = items.value.length < data.total;
    }
  } catch (error) {
    console.error(`Failed to load more ${props.title}:`, error);
  } finally {
    isLoadingMore.value = false;
  }
};

// --- 动画（anime.js v4） ---
// loading spinner：用 ANIME_LOOP.spin 替代 CSS @keyframes spin
function playLoadingSpinner() {
  run(({ animate, loopPresets }) => {
    animate('.loading-state .spinner', { ...loopPresets.spin });
  });
}

// 加载更多按钮内的小 spinner
function playLoadMoreSpinner() {
  run(({ animate, loopPresets }) => {
    animate('.spinner-small', { ...loopPresets.spin });
  });
}

// 页面挂载时获取数据（已加载则跳过）
onMounted(() => {
  if (isLoading.value) {
    nextTick(playLoadingSpinner);
  }
  if (items.value.length === 0) {
    loadItems();
  } else {
    isLoading.value = false;
  }
});

// 监听库变更事件：后端 emit "library-changed" 时 libraryVersion 自增，触发刷新。
// 跳过初始值 0，避免与 onMounted 的首次加载重复。
watch(libraryVersion, (v) => {
  if (v === 0) return;
  loadItems();
});

// loading 状态切换：启动 spinner（列表入场由具体 List 组件内部 playEnter 处理）
watch(
  isLoading,
  (val) => {
    if (val) {
      nextTick(playLoadingSpinner);
    }
  },
  { flush: 'post' }
);

// 加载更多 spinner
watch(
  isLoadingMore,
  (val) => {
    if (val) {
      nextTick(playLoadMoreSpinner);
    }
  },
  { flush: 'post' }
);
</script>

<template>
  <div class="entity-list-page page-container" ref="root">
    <!-- 顶部/底部渐进模糊：贴在 header 下方与底栏上方，消除内容被 .content-wrapper overflow 硬切割的观感 -->
    <GradualBlur
      target="page"
      position="top"
      height="5rem"
      :z-index="-50"
      :strength="2"
      :div-count="4"
      :style="{ top: 'calc(var(--header-height) + var(--safe-area-top))' }"
    />
    <GradualBlur
      target="page"
      position="bottom"
      height="5rem"
      :z-index="-50"
      :strength="2"
      :div-count="4"
      :style="{ bottom: 'calc(var(--bottom-nav-height) + var(--player-bar-height) + var(--safe-area-bottom))' }"
    />

    <div class="page-header">
      <h1 class="page-title">{{ title }}</h1>
      <p class="page-subtitle">共 {{ totalCount || items.length }} {{ unit }}</p>
    </div>

    <div v-if="isLoading" class="loading-state">
      <div class="spinner"></div>
    </div>

    <template v-else>
      <slot name="list" :items="items" v-if="items.length > 0" />

      <div v-if="hasMore" class="load-more">
        <button class="btn btn-secondary" @click="loadMore" :disabled="isLoadingMore">
          <span v-if="isLoadingMore" class="spinner-small"></span>
          {{ isLoadingMore ? '加载中...' : `加载更多 (${items.length}/${totalCount})` }}
        </button>
      </div>

      <div v-else-if="items.length === 0" class="empty-state">
        <slot name="empty-icon" />
        <h3 class="empty-state-title">{{ emptyTitle }}</h3>
        <p class="empty-state-desc">{{ emptyDesc }}</p>
        <router-link to="/music-sources" class="btn btn-primary" style="margin-top: 16px;">
          添加音乐源
        </router-link>
      </div>
    </template>
  </div>
</template>

<style scoped>
.load-more {
  display: flex;
  justify-content: center;
  padding: 24px 0;
}

/* 禁用全局 .spinner 的 CSS spin 动画，改由 anime.js ANIME_LOOP.spin 驱动 */
.loading-state .spinner {
  animation: none;
}

.spinner-small {
  display: inline-block;
  margin-right: 8px;
  vertical-align: middle;
}
</style>
