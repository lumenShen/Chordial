/**
 * 虚拟列表 Composable
 *
 * 高性能渲染大量数据，只渲染可见区域的内容
 * 参考实现：响应式音乐歌单虚拟列表
 *
 * # 性能优化
 * - 只渲染可见区域 + 缓冲区的 DOM 节点
 * - 使用 requestAnimationFrame 节流滚动事件
 * - 二分查找 O(log n) 快速定位可见项
 * - 支持动态高度（通过 itemHeight 函数）
 *
 * # 模式（mode 选项）
 * - 'container'（默认）：列表拥有自己的滚动容器（max-height 限定）。
 *   适用于内嵌场景（弹窗、面板内的列表）。
 * - 'page'：列表使用页面/窗口滚动条，无独立滚动容器。
 *   适用于整页列表（Tracks.vue 全屏曲目列表），与浏览器原生滚动条无缝集成。
 *   通过监听 window scroll 并结合元素 BCR 顶部偏移计算可见区域。
 *
 * # 无限滚动（infiniteScroll 选项）
 * - 当滚动接近底部时（默认 200px 阈值），触发 onLoadMore 回调。
 * - 通过 isLoadingMore 状态防止重复触发。
 * - 同时支持 container 与 page 模式。
 *
 * @param {Ref<Array>} itemsRef - 数据列表
 * @param {Object} options - 配置选项
 * @returns {Object} 虚拟列表状态和计算属性
 */
import { ref, computed, onMounted, onUnmounted, watch, nextTick } from 'vue';
import { perf } from '@/utils/performanceMonitor.js';

export function useVirtualList(itemsRef, options = {}) {
  const {
    itemHeight = 64, // 默认每项高度
    bufferSize = 5, // 缓冲区大小（上下各多渲染几项）
    containerRef = null, // 容器 ref（container 模式下必需，page 模式下用于锚定列表根元素）
    columns = 1, // 列数：>1 时启用网格模式
    mode = 'container', // 'container' | 'page'
    infiniteScroll = false, // 是否启用无限滚动
    loadMoreThreshold = 200, // 距离底部多少 px 触发加载更多
    onLoadMore = null, // 加载更多回调 () => Promise<void> | void
    initialLoadCount = 0, // 初始已加载数量（用于热启动）
  } = options;

  // 滚动状态
  const scrollTop = ref(0);
  const containerHeight = ref(0);
  const rafId = ref(null);
  const windowRafId = ref(null);

  // 无限滚动状态
  const isLoadingMore = ref(false);
  const hasMore = ref(true);
  // 用于 page 模式：列表根元素相对文档顶部的偏移（动态测量）
  const listOffsetTop = ref(0);

  // 计算行高（网格模式下每行高度 = 单项高度）
  const rowHeight = computed(() => {
    if (typeof itemHeight === 'function') {
      // 函数模式：取第一项的高度作为行高近似值
      return itemsRef.value && itemsRef.value.length > 0
        ? itemHeight(itemsRef.value[0], 0)
        : 64;
    }
    return itemHeight;
  });

  // 计算总行数
  const totalRows = computed(() => {
    if (!itemsRef.value || !itemsRef.value.length) return 0;
    return Math.ceil(itemsRef.value.length / columns);
  });

  // 计算总高度
  const totalHeight = computed(() => {
    if (!itemsRef.value) return 0;

    if (columns > 1) {
      // 网格模式：按行计算
      return totalRows.value * rowHeight.value;
    }

    // 单列模式：按项累加
    if (typeof itemHeight === 'function') {
      return itemsRef.value.reduce((sum, item, index) => {
        return sum + itemHeight(item, index);
      }, 0);
    }

    return itemsRef.value.length * itemHeight;
  });

  // 计算前缀和（用于快速定位）
  const prefixSum = computed(() => {
    if (!itemsRef.value) return [];

    if (columns > 1) {
      // 网格模式：按行前缀和
      const sums = new Float64Array(totalRows.value);
      for (let row = 0; row < totalRows.value; row++) {
        sums[row] = (row + 1) * rowHeight.value;
      }
      return sums;
    }

    // 单列模式：按项前缀和
    const sums = new Float64Array(itemsRef.value.length);
    let sum = 0;

    for (let i = 0; i < itemsRef.value.length; i++) {
      const height = typeof itemHeight === 'function'
        ? itemHeight(itemsRef.value[i], i)
        : itemHeight;
      sum += height;
      sums[i] = sum;
    }

    return sums;
  });

  // 二分查找：根据滚动位置找到起始索引 O(log n)
  const findStartIndex = (scrollPos) => {
    if (!prefixSum.value.length) return 0;

    let left = 0;
    let right = prefixSum.value.length - 1;
    let ans = 0;

    while (left <= right) {
      const mid = (left + right) >> 1;
      if (prefixSum.value[mid] <= scrollPos) {
        ans = mid;
        left = mid + 1;
      } else {
        right = mid - 1;
      }
    }

    return ans;
  };

  /**
   * 计算当前可见区域的 [startIdx, endIdx)
   * page 模式下：scrollTop = window.scrollY - listOffsetTop
   * container 模式下：scrollTop = container.scrollTop
   */
  const visibleRange = computed(() => {
    const items = itemsRef.value;
    if (!items || !items.length) return { startIdx: 0, endIdx: 0 };

    // 当前可见滚动偏移（已根据模式归一化）
    const effectiveScrollTop = Math.max(0, scrollTop.value);

    const startIdx = Math.max(0, findStartIndex(effectiveScrollTop) - bufferSize);
    const endIdx = Math.min(
      items.length,
      findStartIndex(effectiveScrollTop + containerHeight.value) + bufferSize,
    );

    return { startIdx, endIdx };
  });

  // 计算可见项
  const visibleItems = computed(() => {
    perf.start('VirtualList.compute');
    if (!itemsRef.value || !itemsRef.value.length) { perf.end('VirtualList.compute'); return []; }

    const { startIdx, endIdx } = visibleRange.value;

    if (columns > 1) {
      // 网格模式：按行渲染
      const startRow = Math.floor(startIdx / columns);
      const endRow = Math.ceil(endIdx / columns);

      const items = [];
      for (let row = startRow; row < endRow; row++) {
        const startCol = row * columns;
        for (let col = 0; col < columns; col++) {
          const idx = startCol + col;
          if (idx >= itemsRef.value.length) break;
          if (idx < startIdx) continue;

          const offsetY = row * rowHeight.value;
          const colWidth = 100 / columns; // 百分比宽度

          items.push({
            item: itemsRef.value[idx],
            index: idx,
            offsetY,
            offsetX: col * colWidth,
            width: colWidth,
            height: rowHeight.value,
            col,
          });
        }
      }

      perf.end('VirtualList.compute');
      return items;
    }

    // 单列模式
    const items = [];
    for (let i = startIdx; i < endIdx; i++) {
      const prevHeight = i > 0 ? prefixSum.value[i - 1] : 0;
      const height = typeof itemHeight === 'function'
        ? itemHeight(itemsRef.value[i], i)
        : itemHeight;

      items.push({
        item: itemsRef.value[i],
        index: i,
        offsetY: prevHeight,
        height,
      });
    }

    perf.end('VirtualList.compute');
    return items;
  });

  // 渲染的 DOM 节点数量
  const poolSize = computed(() => visibleItems.value.length);

  // ── 无限滚动检测 ──────────────────────────────────────
  /**
   * 检测是否需要加载更多。
   * - container 模式：scrollTop + clientHeight >= scrollHeight - threshold
   * - page 模式：window.scrollY + window.innerHeight >= document.documentElement.scrollHeight - threshold
   */
  const checkLoadMore = () => {
    if (!infiniteScroll || !hasMore.value || isLoadingMore.value) return;

    let scrolledToBottom = false;

    if (mode === 'page') {
      const winScroll = window.scrollY || window.pageYOffset || 0;
      const winHeight = window.innerHeight || document.documentElement.clientHeight || 0;
      const docHeight = document.documentElement.scrollHeight || 0;
      scrolledToBottom = winScroll + winHeight >= docHeight - loadMoreThreshold;
    } else {
      const el = containerRef?.value;
      if (!el) return;
      scrolledToBottom = el.scrollTop + el.clientHeight >= el.scrollHeight - loadMoreThreshold;
    }

    if (scrolledToBottom) {
      triggerLoadMore();
    }
  };

  /**
   * 触发加载更多。使用 isLoadingMore 标志防止并发调用。
   */
  const triggerLoadMore = async () => {
    if (isLoadingMore.value || !hasMore.value) return;
    if (typeof onLoadMore !== 'function') return;

    isLoadingMore.value = true;
    try {
      const result = await onLoadMore();
      // 回调可返回 false 表示没有更多数据
      if (result === false) {
        hasMore.value = false;
      }
    } catch (e) {
      console.warn('[useVirtualList] onLoadMore failed:', e);
    } finally {
      isLoadingMore.value = false;
      // 加载完成后再次检查，避免边界条件漏触发
      nextTick(() => checkLoadMore());
    }
  };

  // 外部通知"没有更多" / "有更多"
  const setHasMore = (v) => { hasMore.value = !!v; };

  // 滚动处理（使用 RAF 节流）—— container 模式
  const onScroll = (e) => {
    if (rafId.value) {
      cancelAnimationFrame(rafId.value);
    }

    rafId.value = requestAnimationFrame(() => {
      perf.start('VirtualList.scroll');
      scrollTop.value = e.target.scrollTop;
      perf.end('VirtualList.scroll');
      if (infiniteScroll) checkLoadMore();
    });
  };

  // page 模式的滚动处理：监听 window scroll
  const onPageScroll = () => {
    if (windowRafId.value) {
      cancelAnimationFrame(windowRafId.value);
    }
    windowRafId.value = requestAnimationFrame(() => {
      perf.start('VirtualList.pageScroll');
      const el = containerRef?.value;
      if (el) {
        // 列表根元素的 BCR 顶部（相对视口）
        const rect = el.getBoundingClientRect();
        // 列表相对文档的偏移
        listOffsetTop.value = rect.top + (window.scrollY || 0);
        // 当前可见区域的滚动偏移 = window.scrollY - listOffsetTop
        // 当列表顶部还没进入视口时（listOffsetTop > scrollY），effectiveScrollTop 为负，
        // visibleRange 中 Math.max(0, ...) 会归零，不渲染任何项（实际由上方 padding 撑开）
        scrollTop.value = (window.scrollY || 0) - listOffsetTop.value;
        // 视口高度即为容器高度（page 模式下容器高度 = 视口高度）
        // 但需要考虑列表顶部可能不在视口顶的情况：实际可见高度 = min(viewport, listHeightInViewport)
        containerHeight.value = window.innerHeight || 0;
      }
      perf.end('VirtualList.pageScroll');
      if (infiniteScroll) checkLoadMore();
    });
  };

  // 滚动到指定索引
  const scrollToIndex = (index) => {
    if (mode === 'page') {
      // page 模式：滚动 window 到对应位置
      const el = containerRef?.value;
      if (!el) return;
      let offsetY = 0;
      if (typeof itemHeight === 'function') {
        for (let i = 0; i < index && i < itemsRef.value.length; i++) {
          offsetY += itemHeight(itemsRef.value[i], i);
        }
      } else if (columns > 1) {
        const row = Math.floor(index / columns);
        offsetY = row * rowHeight.value;
      } else {
        offsetY = index * itemHeight;
      }
      const target = listOffsetTop.value + offsetY;
      window.scrollTo({ top: target, behavior: 'auto' });
      return;
    }

    if (!containerRef?.value) return;

    let offsetY = 0;
    if (columns > 1) {
      // 网格模式：索引所在行 * 行高
      const row = Math.floor(index / columns);
      offsetY = row * rowHeight.value;
    } else if (typeof itemHeight === 'function') {
      for (let i = 0; i < index && i < itemsRef.value.length; i++) {
        offsetY += itemHeight(itemsRef.value[i], i);
      }
    } else {
      offsetY = index * itemHeight;
    }

    containerRef.value.scrollTop = offsetY;
  };

  // 滚动到指定偏移
  const scrollTo = (offset) => {
    if (mode === 'page') {
      window.scrollTo({ top: listOffsetTop.value + offset, behavior: 'auto' });
      return;
    }
    if (containerRef?.value) {
      containerRef.value.scrollTop = offset;
    }
  };

  // 更新容器高度
  const updateContainerHeight = () => {
    if (mode === 'page') {
      containerHeight.value = window.innerHeight || 0;
      // 同时更新列表偏移
      const el = containerRef?.value;
      if (el) {
        const rect = el.getBoundingClientRect();
        listOffsetTop.value = rect.top + (window.scrollY || 0);
        // 重新计算 scrollTop（resize 不改变 scrollY，但需重新归一化）
        scrollTop.value = (window.scrollY || 0) - listOffsetTop.value;
      }
    } else if (containerRef?.value) {
      containerHeight.value = containerRef.value.clientHeight;
    }
  };

  // 监听数据变化：若无限滚动启用且数据为空，可触发首次加载（保守策略：由父组件自行控制首次加载）
  watch(
    () => itemsRef.value?.length,
    (newLen, oldLen) => {
      // 数据增加后再次检查是否触底（应对首次加载不足以填满视口的情况）
      if (infiniteScroll && newLen > (oldLen || 0)) {
        nextTick(() => checkLoadMore());
      }
    },
  );

  // 生命周期
  onMounted(() => {
    updateContainerHeight();
    window.addEventListener('resize', updateContainerHeight);

    if (mode === 'page') {
      window.addEventListener('scroll', onPageScroll, { passive: true });
      // 首次触发一次以计算正确的 listOffsetTop
      nextTick(() => {
        onPageScroll();
        if (infiniteScroll) checkLoadMore();
      });
    }
  });

  onUnmounted(() => {
    if (rafId.value) {
      cancelAnimationFrame(rafId.value);
    }
    if (windowRafId.value) {
      cancelAnimationFrame(windowRafId.value);
    }
    window.removeEventListener('resize', updateContainerHeight);
    if (mode === 'page') {
      window.removeEventListener('scroll', onPageScroll);
    }
  });

  return {
    // 状态
    scrollTop,
    containerHeight,
    totalHeight,
    isLoadingMore,
    hasMore,
    listOffsetTop,

    // 计算属性
    visibleItems,
    visibleRange,
    poolSize,
    prefixSum,

    // 方法
    onScroll,
    onPageScroll,
    scrollToIndex,
    scrollTo,
    updateContainerHeight,
    checkLoadMore,
    triggerLoadMore,
    setHasMore,
  };
}

export default useVirtualList;
