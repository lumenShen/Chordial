<script setup lang="ts">
/**
 * LiquidGlass 液态玻璃显示组件
 *
 * 参考 codepen.io/Mikhail-Bespalov/pen/MYwrMNy 与 liquid-glass-react 实现思路：
 * - SVG `feImage` + `feGaussianBlur` + `feDisplacementMap` 组合实现透镜扭曲
 * - 用径向渐变 SVG data URI 作为位移映射图（中心无位移，边缘强位移）
 * - `backdrop-filter: blur() saturate() url(#filterId)` 引用 SVG 滤镜实现背景液态模糊
 *
 * 本组件只是 `useLiquidGlass` composable 的便利封装：
 *   - 内部使用 composable 管理状态
 *   - 渲染 `<LiquidGlassFilter>` 承载 SVG filter 定义
 *   - 适用于需要独立液态玻璃容器的场景（如装饰性玻璃卡片）
 *
 * 对于既有元素的"贴皮"改造（如按钮、导航），推荐直接使用 composable + filter 组件，
 * 避免引入额外包裹层。
 */
import { computed } from "vue";
import { useLiquidGlass } from "@/composables/useLiquidGlass.js";
import LiquidGlassFilter from "./LiquidGlassFilter.vue";

type Tag = "div" | "button" | "span" | "section" | "article";

const props = withDefaults(
  defineProps<{
    /** 宽度，任意合法 CSS 长度 */
    width?: string;
    /** 高度，任意合法 CSS 长度 */
    height?: string;
    /** 圆角，任意合法 CSS 值（如 '50%', '16px', '999px'） */
    borderRadius?: string;
    /**
     * 位移基准强度。
     * - 当 primitiveUnits="objectBoundingBox" 时，该值表示元素尺寸的倍数。
     *   例如 0.006 表示最大偏移元素宽/高的 0.6%，仅产生边缘微妙折射。
     * - 图一风格建议 0.004-0.008；强烈液态扭曲可提高到 0.03-0.05。
     */
    scale?: number;
    /** hover 时的位移强度 */
    hoverScale?: number;
    /** 是否启用 hover 放大效果 */
    interactive?: boolean;
    /** 渲染的 HTML 标签 */
    tag?: Tag;
    /** 背景模糊半径（CSS 像素单位），默认 12px */
    blur?: number;
    /** 位移映射图平滑核（objectBoundingBox 单位），越大越柔和 */
    mapBlur?: number;
    /** 噪声频率，用于增加液态流动感（0 表示关闭） */
    turbulence?: number;
    /**
     * 自定义位移映射图（base64 / data URI / URL）。
     * - 优先使用此 prop；未提供则使用内置 PNG base64
     * - 推荐使用包含噪声纹理的 PNG（如 codepen 原版使用的 displacement map）
     */
    displacementMapSrc?: string;
  }>(),
  {
    width: "20rem",
    height: "20rem",
    borderRadius: "50%",
    /* 图一风格：轻微边缘折射，强磨砂模糊 */
    scale: 0.015,
    hoverScale: 0.03,
    interactive: true,
    tag: "div",
    /* CSS blur() 是像素单位，各向同性，避免细长元素横向拉丝 */
    blur: 12,
    mapBlur: 0.03,
    turbulence: 0,
    displacementMapSrc: "",
  },
);

// 将 props 转为 composable options getter（保持响应式）
const { glassStyle, onEnter, onLeave, filterProps } = useLiquidGlass(() => ({
  blur: props.blur,
  scale: props.scale,
  hoverScale: props.hoverScale,
  interactive: props.interactive,
  mapBlur: props.mapBlur,
  turbulence: props.turbulence,
  displacementMapSrc: props.displacementMapSrc,
  borderRadius: props.borderRadius,
}));

const rootStyle = computed<Record<string, string>>(() => ({
  width: props.width,
  height: props.height,
  ...glassStyle.value,
  display: "block",
}));
</script>

<template>
  <component
    :is="tag"
    class="liquid-glass"
    :style="rootStyle"
    @mouseenter="onEnter"
    @mouseleave="onLeave"
  >
    <slot />
  </component>

  <LiquidGlassFilter v-bind="filterProps" />
</template>

<style scoped>
.liquid-glass {
  position: relative;
  /* button 元素重置 */
  appearance: none;
  -webkit-appearance: none;
  padding: 0;
  margin: 0;
  font: inherit;
  color: inherit;
  /* 图一风格：厚实乳白磨砂玻璃 */
  background: rgba(255, 255, 255, 0.22);
  border: 1px solid rgba(255, 255, 255, 0.45);
  box-shadow:
    0 8px 24px rgba(0, 0, 0, 0.08),
    inset 0 1px 0 rgba(255, 255, 255, 0.55);
  cursor: pointer;
  outline: none;
  box-sizing: border-box;
}

.liquid-glass:focus-visible {
  box-shadow:
    0 0 0 1px rgba(255, 255, 255, 0.45),
    0 0 0 4px rgba(0, 122, 255, 0.5),
    0 8px 24px rgba(0, 0, 0, 0.08),
    inset 0 1px 0 rgba(255, 255, 255, 0.55);
}

@media (prefers-color-scheme: dark) {
  .liquid-glass {
    background: rgba(40, 40, 45, 0.55);
    border-color: rgba(255, 255, 255, 0.18);
    box-shadow:
      0 8px 24px rgba(0, 0, 0, 0.35),
      inset 0 1px 0 rgba(255, 255, 255, 0.12);
  }
}

@media (prefers-reduced-motion: reduce) {
  .liquid-glass {
    transition: none;
  }
}
</style>
