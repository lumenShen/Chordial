/**
 * useResponsiveGrid — 容器感知的响应式网格配置
 *
 * 与 CSS `repeat(auto-fill, minmax(Npx, 1fr))` 保持同一套列数计算逻辑，
 * 但以「实际容器宽度」为基准（而非 window.innerWidth），
 * 因此桌面端带动侧栏/缩放时，虚拟网格的列数与真实渲染区一致。
 *
 * 适用场景：
 *   - 需要 JS 计算列数/行高的虚拟网格（如 ArtistList 虚拟列表）
 *   - 需要把「容器宽度」映射为 UI 尺寸的网格布局
 *
 * 通过 ResizeObserver 监听容器宽度，容器尺寸变化时自动更新列数与行高。
 * 调用方需在模板上用 `:key="layoutKey"` 强制重建依赖列数的虚拟列表容器。
 *
 * @example
 * const rootRef = useTemplateRef('root');
 * const { columns, itemHeight, layoutKey } = useResponsiveGrid(rootRef, {
 *   cardMinWidth: 160,  // 与 CSS minmax 的第一参一致
 *   gap: 24,            // 与 CSS grid gap 一致
 *   extraHeight: 76,    // 卡片内封面之下到行底的额外高度（文字区 + 行距）
 * });
 */
import { ref, computed, onMounted, onBeforeUnmount } from 'vue';

export function useResponsiveGrid(containerRef, options = {}) {
  const {
    cardMinWidth = 160,
    gap = 24,
    extraHeight = 76,
    minColumns = 2,
  } = options;

  const containerWidth = ref(0);
  let observer = null;

  // 列数：与 CSS auto-fill 公式一致（Math.floor((w + gap) / (minWidth + gap))）
  const columns = computed(() => {
    const w = containerWidth.value;
    if (w <= 0) return 5;
    const cols = Math.floor((w + gap) / (cardMinWidth + gap));
    return Math.max(minColumns, cols);
  });

  // 行高：卡片宽度 = 容器宽 / 列数，加上封面与文字/行距的额外高度
  const itemHeight = computed(() => {
    const w = containerWidth.value;
    const cols = columns.value;
    const cardWidth = cols > 0 ? w / cols : cardMinWidth;
    return cardWidth + extraHeight;
  });

  // 布局键：列数或行高变化时，调用方据此重建虚拟列表容器
  const layoutKey = computed(() => `${columns.value}_${Math.round(itemHeight.value)}`);

  onMounted(() => {
    const el = containerRef?.value;
    if (!el) return;
    containerWidth.value = el.clientWidth;
    if (typeof ResizeObserver !== 'undefined') {
      observer = new ResizeObserver((entries) => {
        for (const entry of entries) {
          containerWidth.value = entry.contentRect.width;
        }
      });
      observer.observe(el);
    }
  });

  onBeforeUnmount(() => {
    observer?.disconnect();
    observer = null;
  });

  return { containerWidth, columns, itemHeight, layoutKey };
}

export default useResponsiveGrid;