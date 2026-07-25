/**
 * useLiquidGlass - 液态玻璃效果 composable
 *
 * 参考 https://liquid-glass-vue.netlify.app/ 与 liquid-glass-react 实现思路：
 * - CSS `backdrop-filter: blur() saturate()` 提供各向同性磨砂模糊
 * - SVG `<filter>` + `feDisplacementMap` 提供边缘折射/色散
 * - `aberration` 通过 RGB 通道分离位移产生色差（chromatic aberration）
 * - `elasticity` 通过鼠标跟随 + rAF 弹性回弹实现"玻璃向光标延伸"
 * - hover 时通过 requestAnimationFrame 平滑过渡 displacement scale
 *
 * 参数语义（与 liquid-glass-react 对齐）：
 * - `displacementScale`：位移量表（像素值，userSpaceOnUse），控制边缘变形强度
 * - `blurAmount`：模糊量（0-1 范围，映射为 0-100px），控制背景模糊强度
 * - `saturation`：饱和度百分比
 * - `aberration`：像差强度（像素值），控制 RGB 通道分离强度
 * - `elasticity`：弹性（0-1），控制玻璃向光标延伸的程度
 * - `refractionMode`：折射模式（'standard' 标准 | 'polar' 极地）
 *
 * 用法：
 * ```vue
 * <script setup>
 * import { useLiquidGlass, LIQUID_GLASS_PRESETS } from '@/composables/useLiquidGlass.js';
 * import LiquidGlassFilter from '@/components/ui/LiquidGlassFilter.vue';
 *
 * const { glassStyle, onMouseMove, onEnter, onLeave, filterProps } = useLiquidGlass(
 *   LIQUID_GLASS_PRESETS.user
 * );
 * </script>
 *
 * <template>
 *   <button :style="glassStyle" @mousemove="onMouseMove" @mouseenter="onEnter" @mouseleave="onLeave">...</button>
 *   <LiquidGlassFilter v-bind="filterProps" />
 * </template>
 * ```
 *
 * 设计要点：
 *   - composable 负责：状态管理、样式计算、hover scale 动画、elasticity 鼠标跟随
 *   - LiquidGlassFilter 负责：渲染 SVG `<filter>` 节点，支持 aberration 多通道位移
 *   - 通过 `useId` 保证多实例 filter id 唯一，避免 SVG 引用冲突
 *   - `currentScale` 为响应式 ref：aberration 模式下 filter 组件需 watch 同步多个 feDisplacementMap
 */
import { computed, onBeforeUnmount, ref, useId, watch } from 'vue';
// 内置位移映射图（含噪声纹理的 PNG，data URI 形式）
// 来源：codepen.io/Mikhail-Bespalov/pen/MYwrMNy 原版 displacement map
import liquidGlassMapBase64 from '@/assets/liquidGlassMapBase64.txt?raw';

/**
 * 预设参数集合
 *
 * `user` 预设为用户指定的标准配置：
 *   折射模式=标准, 位移量表=150, 模糊量=0.1, 饱和度=130%,
 *   像差强度=2, 弹性=0.35
 */
export const LIQUID_GLASS_PRESETS = {
  user: {
    refractionMode: 'standard',
    displacementScale: 150,
    blurAmount: 0.1,
    saturation: 130,
    aberration: 2,
    elasticity: 0.35,
    interactive: true,
  },
};

/**
 * @typedef {Object} LiquidGlassOptions
 * @property {'standard'|'polar'} [refractionMode='standard']  折射模式（控制折射计算方法）
 * @property {number} [displacementScale]                       位移量表（像素值，控制边缘变形强度）
 * @property {number} [scale=0.015]                             位移基准强度（objectBoundingBox 倍数，displacementScale 优先）
 * @property {number} [hoverScale]                              hover 时的位移强度（默认 scale * 1.5）
 * @property {number} [blur=12]                                 背景模糊半径（CSS 像素）
 * @property {number} [blurAmount]                              模糊量（0-1，优先于 blur，映射为 blurAmount*100 px）
 * @property {boolean} [saturate=true]                          是否启用饱和度增强
 * @property {number} [saturation=180]                          饱和度百分比（控制背景色彩饱和度）
 * @property {number} [aberration=0]                            像差强度（像素值，控制 RGB 通道分离强度）
 * @property {number} [elasticity=0]                            弹性（0-1，控制玻璃向光标延伸的程度）
 * @property {boolean} [interactive=true]                       是否启用 hover 放大效果
 * @property {number} [mapBlur=0.03]                            位移映射图平滑核
 * @property {number} [turbulence=0]                            噪声频率（0 关闭）
 * @property {string} [displacementMapSrc]                      自定义位移映射图（data URI / URL）
 * @property {string} [borderRadius]                            圆角（任意 CSS 值）
 * @property {string} [baseTransform]                           基础 transform（elasticity 会在此基础上叠加）
 */

/**
 * 极地模式位移图（极坐标渐变，中心清晰边缘强折射）
 */
function buildPolarDisplacementMap() {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" preserveAspectRatio="none">
  <defs>
    <radialGradient id="p" cx="50%" cy="50%" r="50%">
      <stop offset="0%" stop-color="#808080"/>
      <stop offset="35%" stop-color="#808080"/>
      <stop offset="100%" stop-color="#ffffff"/>
    </radialGradient>
  </defs>
  <rect width="256" height="256" fill="url(#p)"/>
</svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}

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
 *   onMouseMove: (e: MouseEvent) => void,
 *   filterProps: import('vue').ComputedRef<Object>,
 *   displacementMapUrl: import('vue').ComputedRef<string>,
 *   setDisplacementMapRef: (el: SVGFEDisplacementMapElement | null) => void,
 *   currentScale: import('vue').Ref<number>,
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

  const dispMapEls = ref([]);
  let animFrame = null;

  // ── 响应式 scale：aberration 模式下 filter 组件需 watch 同步多通道 ──
  // 基准 scale（G 通道），R = scale + aberration, B = scale - aberration
  const currentScale = ref(0);

  // ── elasticity 鼠标跟随状态 ──
  const elasticX = ref(0);
  const elasticY = ref(0);
  let targetX = 0;
  let targetY = 0;
  let elasticFrame = null;

  // 位移映射图：根据折射模式选择
  const displacementMapUrl = computed(() => {
    const { refractionMode, displacementMapSrc } = opts.value;
    if (displacementMapSrc) return displacementMapSrc;
    if (refractionMode === 'polar') return buildPolarDisplacementMap();
    return liquidGlassMapBase64 || buildFallbackRadialGradient();
  });

  // 实际基准 scale 值（G 通道，objectBoundingBox 倍数）
  // displacementScale 是 liquid-glass-react 风格的归一化值（0-200），
  // 转换为 objectBoundingBox 倍数：/ 1000（150 → 0.15，对 60px 元素约 9px 位移）
  const resolvedScale = computed(() => {
    const { displacementScale, scale = 0.015 } = opts.value;
    return displacementScale != null ? displacementScale / 1000 : scale;
  });

  // hover 时的 scale
  const resolvedHoverScale = computed(() => {
    const { displacementScale, hoverScale, scale = 0.015 } = opts.value;
    if (hoverScale != null) return hoverScale;
    if (displacementScale != null) return (displacementScale * 1.5) / 1000;
    return scale * 2;
  });

  // 模糊量：blurAmount 优先（0-1 映射到 0-100px）
  const resolvedBlur = computed(() => {
    const { blur = 12, blurAmount } = opts.value;
    if (blurAmount != null) return blurAmount * 100;
    return blur;
  });

  /**
   * 计算 backdrop-filter 字符串。
   * - blur + saturate 由 CSS 负责（各向同性像素模糊）
   * - url(#filterId) 引用 SVG filter 做边缘折射 + 像差
   */
  const backdropFilter = computed(() => {
    const { saturate = true, saturation = 180 } = opts.value;
    const parts = [`blur(${resolvedBlur.value}px)`];
    if (saturate) parts.push(`saturate(${saturation}%)`);
    parts.push(`url(#${filterId})`);
    return parts.join(' ');
  });

  /**
   * 绑定到目标元素的 inline style。
   * 包含 backdrop-filter、border-radius、elasticity transform。
   */
  const glassStyle = computed(() => {
    const { borderRadius, baseTransform } = opts.value;
    const style = {
      backdropFilter: backdropFilter.value,
      WebkitBackdropFilter: backdropFilter.value,
    };
    if (borderRadius) style.borderRadius = borderRadius;
    // elasticity transform：在 baseTransform 基础上叠加
    const ex = elasticX.value;
    const ey = elasticY.value;
    if (baseTransform || ex !== 0 || ey !== 0) {
      const parts = [];
      if (baseTransform) parts.push(baseTransform);
      if (ex !== 0 || ey !== 0) parts.push(`translate(${ex.toFixed(2)}px, ${ey.toFixed(2)}px)`);
      style.transform = parts.join(' ');
    }
    return style;
  });

  /**
   * 传给 <LiquidGlassFilter> 组件的 props。
   * 包含 registerDisplacementMap 回调，让 filter 组件可以将 feDisplacementMap
   * 元素注册回 composable，使 hover 动画可以驱动 attribute。
   * aberration 保持原值（像素强度），filter 组件内部转换为 obb 倍数。
   */
  const filterProps = computed(() => ({
    filterId,
    displacementMapUrl: displacementMapUrl.value,
    scale: currentScale.value,
    aberration: opts.value.aberration ?? 0,
    mapBlur: opts.value.mapBlur ?? 0.03,
    turbulence: opts.value.turbulence ?? 0,
    registerDisplacementMap: setDisplacementMapRef,
  }));

  /**
   * 注册 SVG <feDisplacementMap> 元素引用，用于 hover 时动画 scale attribute。
   * 由 LiquidGlassFilter 组件通过 registerDisplacementMap prop 回调调用。
   * 支持注册单个元素、数组（aberration 模式下 R/G/B 三个 feDisplacementMap）或 null（清空）。
   */
  function setDisplacementMapRef(el) {
    if (el === null) {
      // 清空所有引用（卸载时）
      dispMapEls.value = [];
    } else if (Array.isArray(el)) {
      // 数组批量注册
      dispMapEls.value = el.filter(Boolean);
      dispMapEls.value.forEach((e) => e.setAttribute('scale', String(currentScale.value)));
    } else if (el && !dispMapEls.value.includes(el)) {
      dispMapEls.value.push(el);
      // 确保初始 scale attribute 已设置（Vue 对 SVG filter primitive 的 attribute 绑定不稳定）
      el.setAttribute('scale', String(currentScale.value));
    }
  }

  /**
   * 平滑过渡 displacement scale（驱动所有已注册的 feDisplacementMap）。
   * SVG filter primitive 的 scale 是 attribute 而非 CSS 属性，
   * 无法用 CSS transition 动画，需手动 rAF 插值。
   * 通过更新 currentScale ref，filter 组件 watch 后同步 aberration 通道。
   */
  function animateScale(target) {
    if (animFrame != null) cancelAnimationFrame(animFrame);
    const start = currentScale.value;
    const duration = 300;
    const startTime = performance.now();
    const step = (now) => {
      const t = Math.min(1, (now - startTime) / duration);
      // ease-out cubic
      const eased = 1 - Math.pow(1 - t, 3);
      const v = start + (target - start) * eased;
      currentScale.value = v;
      // 直接同步 attribute（避免依赖 watch 的延迟）
      dispMapEls.value.forEach((el) => el.setAttribute('scale', String(v)));
      if (t < 1) animFrame = requestAnimationFrame(step);
      else animFrame = null;
    };
    animFrame = requestAnimationFrame(step);
  }

  function onEnter() {
    if (opts.value.interactive === false) return;
    animateScale(resolvedHoverScale.value);
  }

  function onLeave() {
    if (opts.value.interactive === false) return;
    animateScale(resolvedScale.value);
    // elasticity 回弹
    targetX = 0;
    targetY = 0;
    startElasticAnimation();
  }

  /**
   * elasticity 鼠标跟随：根据光标相对元素中心的位置，应用轻微 transform 偏移，
   * 营造"玻璃向光标延伸"的视觉效果。
   * 偏移量 = (光标偏移比例) * elasticity * 30px
   */
  function onMouseMove(e) {
    const { elasticity = 0 } = opts.value;
    if (elasticity <= 0) return;
    const el = e.currentTarget;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const cx = rect.left + rect.width / 2;
    const cy = rect.top + rect.height / 2;
    const dx = (e.clientX - cx) / rect.width;   // -0.5 ~ 0.5
    const dy = (e.clientY - cy) / rect.height;  // -0.5 ~ 0.5
    targetX = dx * elasticity * 30;
    targetY = dy * elasticity * 30;
    startElasticAnimation();
  }

  /**
   * elasticity 平滑过渡（ease-out cubic，与 scale 动画一致）。
   */
  function startElasticAnimation() {
    if (elasticFrame != null) cancelAnimationFrame(elasticFrame);
    const startX = elasticX.value;
    const startY = elasticY.value;
    const duration = 200;
    const startTime = performance.now();
    const step = (now) => {
      const t = Math.min(1, (now - startTime) / duration);
      const eased = 1 - Math.pow(1 - t, 3);
      elasticX.value = startX + (targetX - startX) * eased;
      elasticY.value = startY + (targetY - startY) * eased;
      if (t < 1) elasticFrame = requestAnimationFrame(step);
      else elasticFrame = null;
    };
    elasticFrame = requestAnimationFrame(step);
  }

  // 初始化 currentScale 为基准值
  currentScale.value = resolvedScale.value;

  // 当基准 scale 变化时同步 currentScale 与 attribute（响应式 options 支持）
  watch(resolvedScale, (v) => {
    currentScale.value = v;
    dispMapEls.value.forEach((el) => el.setAttribute('scale', String(v)));
  });

  onBeforeUnmount(() => {
    if (animFrame != null) cancelAnimationFrame(animFrame);
    if (elasticFrame != null) cancelAnimationFrame(elasticFrame);
  });

  return {
    filterId,
    glassStyle,
    onEnter,
    onLeave,
    onMouseMove,
    filterProps,
    displacementMapUrl,
    setDisplacementMapRef,
    currentScale,
  };
}
