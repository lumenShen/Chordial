/**
 * 封面图片加载 Composable
 * 使用 ResourceManager 按需加载封面图片
 *
 * # 性能说明
 * - acquireCoverResource 仅同步构造 chordial:// URL（真正的图片下载由
 *   浏览器 <img> 发起，浏览器自身管理并发连接），因此批量加载不做
 *   JS 层并发限制——人为限流反而会把同步操作串行化，造成排队延迟。
 * - 优化 watch 监听，避免深度监听大对象。
 * - 自动管理资源生命周期，组件卸载时释放资源并跳过排队中的请求。
 */
import { ref, onMounted, onUnmounted, watch, computed } from 'vue';

/**
 * 从对象获取封面资源
 * 优先使用 getCoverResource 或 acquireCoverResource 方法，其次使用 coverData
 * @param {Object} item - 专辑/曲目/歌手对象
 * @param {string} size - 图片尺寸
 * @returns {Promise<{url: string, release: Function}|null>}
 */
async function acquireCoverResource(item, size = 'medium') {
  if (!item) return null;

  // 优先级1: 使用 acquireCoverResource 方法（统一接口）
  if (typeof item.acquireCoverResource === 'function') {
    return await item.acquireCoverResource(size);
  }

  // 优先级2: 使用 getCoverResource 方法
  if (typeof item.getCoverResource === 'function') {
    return await item.getCoverResource(size);
  }

  // 优先级3: 使用 coverData（Data URL，不需要释放）
  if (item.coverData) {
    return {
      url: item.coverData,
      release: () => {} // Data URL 不需要释放
    };
  }

  // 优先级4: 使用 albumCoverData（Track 对象）
  if (item.albumCoverData) {
    return {
      url: item.albumCoverData,
      release: () => {} // Data URL 不需要释放
    };
  }

  return null;
}

/**
 * 加载专辑或歌手封面图片
 * @param {Object} item - 专辑或歌手对象 (AlbumSummary, Album, ArtistSummary, Artist)
 * @param {string} size - 图片尺寸 ('small', 'medium', 'large')
 * @returns {Object} { coverUrl, isLoading, error, release }
 */
export function useCoverImage(item, size = 'medium') {
  const coverUrl = ref('');
  const isLoading = ref(false);
  const error = ref(null);
  let releaseFn = null;
  let isLoadingCover = false;

  const loadCover = async () => {
    if (!item.value) return;
    if (isLoadingCover) {
      return;
    }

    isLoadingCover = true;
    isLoading.value = true;
    error.value = null;

    try {
      const resource = await acquireCoverResource(item.value, size);

      if (resource && resource.url) {
        if (releaseFn) {
          releaseFn();
          releaseFn = null;
        }

        coverUrl.value = resource.url;
        releaseFn = typeof resource.release === 'function'
          ? () => resource.release()
          : null;
      }
    } catch (err) {
      console.warn('Failed to load cover:', err);
      error.value = err;
    } finally {
      isLoadingCover = false;
      isLoading.value = false;
    }
  };

  const releaseCover = () => {
    if (releaseFn) {
      releaseFn();
      releaseFn = null;
    }
  };

  let lastItemId = null;

  onMounted(() => {
    lastItemId = item.value?.id;
    loadCover();
  });

  watch(() => item.value?.id, (newId, oldId) => {
    if (newId !== lastItemId) {
      lastItemId = newId;
      loadCover();
    }
  }, { immediate: false });

  onUnmounted(() => {
    releaseCover();
  });

  return {
    coverUrl,
    isLoading,
    error,
    reload: loadCover,
    release: releaseCover
  };
}

/**
 * 批量加载封面图片（用于列表）
 * @param {Ref<Array>} itemsRef - 专辑或歌手列表的 ref
 * @param {string} size - 图片尺寸
 * @returns {Object} { coverUrls, isLoading }
 */
export function useCoverImages(itemsRef, size = 'medium') {
  const coverUrls = ref(new Map());
  const isLoading = ref(false);
  const releaseFns = new Map();
  // 组件卸载标志：阻止卸载后仍在进行的资源获取继续写入状态
  // （acquireCoverResource 含 await，卸载后应跳过资源获取与状态写入）
  let disposed = false;

  const loadCovers = async () => {
    if (!itemsRef.value || itemsRef.value.length === 0) return;

    isLoading.value = true;

    // 批量构造封面 URL（acquireCoverResource 仅同步构造 chordial:// URL，
    // 真正的图片下载并发由浏览器管理），先收集到临时 Map，最后一次性替换
    // （避免 N+1 次重渲染）。
    const newCoverUrls = new Map();
    const newReleaseFns = new Map();

    const promises = itemsRef.value.map(async (item) => {
      if (!item) return;

      try {
        // 卸载检查：若组件已卸载，跳过资源获取
        if (disposed) return;
        // 使用统一的资源获取函数（构造 chordial:// URL）
        const resource = await acquireCoverResource(item, size);
        // 再次检查：await 期间可能已卸载，避免写入已卸载组件的 Map
        if (disposed) {
          resource?.release?.();
          return;
        }
        if (resource && resource.url) {
          newCoverUrls.set(item.id, resource.url);
          newReleaseFns.set(item.id, resource.release);
        }
      } catch (err) {
        console.warn(`Failed to load cover for ${item.id}:`, err);
      }
    });

    await Promise.all(promises);

    // 卸载检查：避免卸载后写入响应式状态触发无意义更新
    if (disposed) {
      newReleaseFns.forEach((fn) => fn?.());
      return;
    }

    // 释放旧的资源
    releaseFns.forEach((release) => release());
    releaseFns.clear();

    // 一次性替换整个 Map，只触发一次响应式更新
    coverUrls.value = newCoverUrls;
    // 更新 releaseFns 引用
    releaseFns.clear();
    newReleaseFns.forEach((fn, id) => releaseFns.set(id, fn));

    isLoading.value = false;
  };

  const releaseAll = () => {
    releaseFns.forEach((release) => release());
    releaseFns.clear();
    coverUrls.value.clear();
  };

  // 监听列表长度和 ID 变化，避免深度监听大对象
  const itemsKey = computed(() => {
    const items = itemsRef.value;
    if (!items) return '';
    return `${items.length}_${items.map(i => i?.id).join(',')}`;
  });
  watch(itemsKey, () => {
    loadCovers();
  });

  // 组件挂载时加载封面
  onMounted(() => {
    loadCovers();
  });

  // 组件卸载时释放所有资源，并阻止队列中等待的请求继续执行
  onUnmounted(() => {
    disposed = true;
    releaseAll();
  });

  return {
    coverUrls,
    isLoading,
    reload: loadCovers,
    releaseAll
  };
}

export default useCoverImage;
