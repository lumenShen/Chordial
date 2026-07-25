/**
 * useLiquidGlass - 液态玻璃效果 composable
 *
 * 参考 https://liquid-glass-vue.netlify.app/ 与 liquid-glass-react 实现思路：
 * - CSS `backdrop-filter: blur() saturate()` 提供各向同性磨砂模糊
 * - SVG `<filter>` + `feDisplacementMap` 提供边缘折射/色散
 * - 径向渐变 mask 让色散仅出现在边缘，中心保持清晰
 * - hover 时通过 requestAnimationFrame 平滑过渡 displacement scale
 *
 * 用法：
 * ```vue
 * <script setup>
 * import { useLiquidGlass } from '@/composables/useLiquidGlass.js';
 * import LiquidGlassFilter from '@/components/ui/LiquidGlassFilter.vue';
 *
 * const { filterId, glassStyle, onEnter, onLeave, filterProps } = useLiquidGlass({
 *   blur: 12,
 *   scale: 0.015,
 *   hoverScale: 0.03,
 * });
 * </script>
 *
 * <template>
 *   <button :style="glassStyle" @mouseenter="onEnter" @mouseleave="onLeave">...</button>
 *   <LiquidGlassFilter v-bind="filterProps" />
 * </template>
 * ```
 *
 * 设计要点：
 *   - composable 负责：状态管理、样式计算、hover scale 动画
 *   - LiquidGlassFilter 负责：渲染 SVG `<filter>` 节点，并通过 `registerDisplacementMap`
 *     回调将 `<feDisplacementMap>` 元素注册回 composable，使 hover 动画可以驱动 attribute
 *   - 通过 `useId` 保证多实例 filter id 唯一，避免 SVG 引用冲突
 */
import { computed, onBeforeUnmount, ref, useId, watch } from 'vue';
// 内置位移映射图（含噪声纹理的 PNG，data URI 形式）
// 来源：codepen.io/Mikhail-Bespalov/pen/MYwrMNy 原版 displacement map
import liquidGlassMapBase64 from '@/assets/liquidGlassMapBase64.txt?raw';

/**
 * @typedef {Object} LiquidGlassOptions
 * @property {number} [blur=12]             背景模糊半径（CSS 像素）
 * @property {number} [scale=0.015]         位移基准强度（objectBoundingBox 倍数）
 * @property {number} [hoverScale=0.03]     hover 时的位移强度
 * @property {boolean} [interactive=true]   是否启用 hover 放大效果
 * @property {number} [mapBlur=0.03]        位移映射图平滑核（objectBoundingBox 单位）
 * @property {number} [turbulence=0]        噪声频率（0 关闭）
 * @property {string} [displacementMapSrc]  自定义位移映射图（data URI / URL）
 * @property {string} [borderRadius]        圆角（任意 CSS 值）
 * @property {boolean} [saturate=true]      是否启用饱和度增强
 * @property {number} [saturation=180]      饱和度百分比
 */

/**
 * 径向渐变 fallback 位移图（当内置 PNG 加载失败时使用）
 */
function buildFallbackRadialGradient() {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" preserveAspectRatio="none">
  <defs>
    <radialGradient id="g" cx="50%" cy="50%" r="50%">
      <stop offset="0%" stop-color="#808080"/>
      <stop offset="55%" stop-color="#808080"/>
      <stop offset="100%" stop-color="#ffffff"/>
    </radialGradient>
  </defs>
  <rect width="256" height="256" fill="url(#g)"/>
</svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}

/**
 * 创建液态玻璃效果实例。
 *
 * @param {LiquidGlassOptions|() => LiquidGlassOptions} optionsOrGetter
 * @returns {{
 *   filterId: string,
 *   glassStyle: import('vue').ComputedRef<Record<string, string>>,
 *   onEnter: () => void,
 *   onLeave: () => void,
 *   filterProps: import('vue').ComputedRef<Object>,
 *   displacementMapUrl: import('vue').ComputedRef<string>,
 *   setDisplacementMapRef: (el: SVGFEDisplacementMapElement | null) => void,
 * }}
 */
export function useLiquidGlass(optionsOrGetter = {}) {
  // 支持响应式 getter 或静态对象
  const resolveOpts = typeof optionsOrGetter === 'function'
    ? optionsOrGetter
    : () => optionsOrGetter;

  // Vue 3.5+ useId：保证多实例 SVG filter id 唯一
  const filterId = `liquid-glass-${useId()}`;

  // 响应式 options：getter 模式下会随依赖更新
  const opts = computed(resolveOpts);

  const dispMapEl = ref(null);
  let animFrame = null;

  // 位移映射图：优先用户自定义 > 内置 PNG > 径向渐变 fallback
  const displacementMapUrl = computed(() => {
    const src = opts.value.displacementMapSrc;
    return src || liquidGlassMapBase64 || buildFallbackRadialGradient();
  });

  /**
   * 计算 backdrop-filter 字符串。
   * - blur + saturate 由 CSS 负责（各向同性像素模糊）
   * - url(#filterId) 引用 SVG filter 做边缘折射
   */
  const backdropFilter = computed(() => {
    const { blur = 12, saturate = true, saturation = 180 } = opts.value;
    const parts = [`blur(${blur}px)`];
    if (saturate) parts.push(`saturate(${saturation}%)`);
    parts.push(`url(#${filterId})`);
    return parts.join(' ');
  });

  /**
   * 绑定到目标元素的 inline style。
   */
  const glassStyle = computed(() => {
    const { borderRadius } = opts.value;
    const style = {
      backdropFilter: backdropFilter.value,
      WebkitBackdropFilter: backdropFilter.value,
    };
    if (borderRadius) style.borderRadius = borderRadius;
    return style;
  });

  /**
   * 传给 <LiquidGlassFilter> 组件的 props。
   * 包含 registerDisplacementMap 回调，让 filter 组件可以将 feDisplacementMap
   * 元素注册回 composable，使 hover 动画可以驱动 SVG attribute。
   */
  const filterProps = computed(() => ({
    filterId,
    displacementMapUrl: displacementMapUrl.value,
    scale: opts.value.scale ?? 0.015,
    mapBlur: opts.value.mapBlur ?? 0.03,
    turbulence: opts.value.turbulence ?? 0,
    registerDisplacementMap: setDisplacementMapRef,
  }));

  /**
   * 注册 SVG <feDisplacementMap> 元素引用，用于 hover 时动画 scale attribute。
   * 由 LiquidGlassFilter 组件通过 registerDisplacementMap prop 回调调用。
   */
  function setDisplacementMapRef(el) {
    dispMapEl.value = el;
    if (el) {
      // 确保初始 scale attribute 已设置（Vue 对 SVG filter primitive 的 attribute 绑定不稳定）
      el.setAttribute('scale', String(opts.value.scale ?? 0.015));
    }
  }

  /**
   * 平滑过渡 displacement scale。
   * SVG filter primitive 的 scale 是 attribute 而非 CSS 属性，
   * 无法用 CSS transition 动画，需手动 rAF 插值。
   */
  function animateScale(target) {
    if (animFrame != null) cancelAnimationFrame(animFrame);
    const el = dispMapEl.value;
    if (!el) return;
    const start = parseFloat(el.getAttribute('scale') ?? String(opts.value.scale ?? 0.015));
    const duration = 300;
    const startTime = performance.now();
    const step = (now) => {
      const t = Math.min(1, (now - startTime) / duration);
      // ease-out cubic
      const eased = 1 - Math.pow(1 - t, 3);
      const v = start + (target - start) * eased;
      el.setAttribute('scale', String(v));
      if (t < 1) animFrame = requestAnimationFrame(step);
      else animFrame = null;
    };
    animFrame = requestAnimationFrame(step);
  }

  function onEnter() {
    if (opts.value.interactive === false) return;
    animateScale(opts.value.hoverScale ?? 0.03);
  }

  function onLeave() {
    if (opts.value.interactive === false) return;
    animateScale(opts.value.scale ?? 0.015);
  }

  // 当 scale 选项变化时同步 attribute（响应式 options 支持）
  watch(() => opts.value.scale, (v) => {
    if (dispMapEl.value && v != null) {
      dispMapEl.value.setAttribute('scale', String(v));
    }
  });

  onBeforeUnmount(() => {
    if (animFrame != null) cancelAnimationFrame(animFrame);
  });

  return {
    filterId,
    glassStyle,
    onEnter,
    onLeave,
    filterProps,
    displacementMapUrl,
    setDisplacementMapRef,
  };
}
