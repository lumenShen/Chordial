<script setup lang="ts">
/**
 * LiquidGlass 液态玻璃组件（liquid-glass-react Vue 移植版）
 *
 * 参考：https://github.com/rdev/liquid-glass-react
 *
 * 特性：
 * - 4 种折射模式：standard / polar / prominent / shader
 * - RGB 三通道色散（chromatic aberration）
 * - 鼠标追踪 + 弹性形变（directional scale + elastic translation）
 * - 边框渐变（由 mouseOffset 驱动方向）
 * - hover / active 高光层
 * - overLight 亮色背景模式
 *
 * 实现分层：
 * - composable（useLiquidGlass）：响应式状态、鼠标追踪、计算属性
 * - 组件：渲染 SVG filter 模板 + 多层叠加 DOM
 *
 * 兼容性：
 * - Chromium 76+ / Safari 18+ 完整支持 backdrop-filter: url(#id)
 * - Firefox 自动降级（跳过 SVG filter，仅保留 backdrop-filter blur/saturate）
 * - Tauri WebView2 / WKWebView 完整支持
 */
import type { ComponentPublicInstance } from "vue";
import { computed, ref, useAttrs, watch } from "vue";
import { useLiquidGlass } from "@/composables/useLiquidGlass.js";

type Tag = "div" | "button" | "span" | "section" | "article";
type Mode = "standard" | "polar" | "prominent" | "shader";

const props = withDefaults(
  defineProps<{
    /** 渲染的 HTML 标签 */
    tag?: Tag;
    /** 宽度，任意合法 CSS 长度（仅当未通过 class/style 覆盖时生效） */
    width?: string;
    /** 高度，任意合法 CSS 长度 */
    height?: string;
    /** 圆角 px（liquid-glass-react 风格，默认 999 即胶囊形） */
    cornerRadius?: number;
    /** 内边距 */
    padding?: string;
    /** 位移强度（默认 70，对齐原库） */
    displacementScale?: number;
    /** 模糊量 0-1（实际 blur = (overLight?12:4) + blurAmount*32 px） */
    blurAmount?: number;
    /** 饱和度百分比（默认 140） */
    saturation?: number;
    /** 色散强度（默认 2） */
    aberrationIntensity?: number;
    /** 弹性系数 0-1（默认 0.15） */
    elasticity?: number;
    /** 是否浮于亮色背景 */
    overLight?: boolean;
    /** 折射模式 */
    mode?: Mode;
    /** 是否启用鼠标弹性效果 */
    interactive?: boolean;
    /** 自定义位移贴图（覆盖内置） */
    displacementMapSrc?: string;
    /** 是否显示边框层 */
    borderless?: boolean;
    /** 是否禁用 hover 高光层 */
    noHoverHighlight?: boolean;
    /** 鼠标感应容器（外部 ref，由父组件传入；不传则用玻璃自身） */
    mouseContainer?: HTMLElement | null;
  }>(),
  {
    tag: "div",
    width: "",
    height: "",
    cornerRadius: 999,
    padding: "",
    displacementScale: 70,
    blurAmount: 0.0625,
    saturation: 140,
    aberrationIntensity: 2,
    elasticity: 0.15,
    overLight: false,
    mode: "standard",
    interactive: true,
    displacementMapSrc: "",
    borderless: false,
    noHoverHighlight: false,
    mouseContainer: null,
  },
);

const emit = defineEmits<{
  (e: "click", ev: MouseEvent): void;
}>();

// mouseContainer 作为 ref 传入 composable（响应式）
const mouseContainerRef = ref(props.mouseContainer);
watch(
  () => props.mouseContainer,
  (v) => { mouseContainerRef.value = v; },
);

const lg = useLiquidGlass({
  displacementScale: props.displacementScale,
  blurAmount: props.blurAmount,
  saturation: props.saturation,
  aberrationIntensity: props.aberrationIntensity,
  elasticity: props.elasticity,
  cornerRadius: props.cornerRadius,
  overLight: props.overLight,
  mode: props.mode,
  interactive: props.interactive,
  mouseContainer: mouseContainerRef,
});

// 同步 prop 变化到 composable 内部 ref
watch(() => props.displacementScale, (v) => { lg.displacementScale.value = v; });
watch(() => props.blurAmount, (v) => { lg.blurAmount.value = v; });
watch(() => props.saturation, (v) => { lg.saturation.value = v; });
watch(() => props.aberrationIntensity, (v) => { lg.aberrationIntensity.value = v; });
watch(() => props.elasticity, (v) => { lg.elasticity.value = v; });
watch(() => props.overLight, (v) => { lg.overLight.value = v; });
watch(() => props.mode, (v) => { lg.mode.value = v; });
watch(() => props.interactive, (v) => { lg.interactive.value = v; });

// 位移贴图：优先使用 prop 自定义值
const displacementMapUrl = computed(
  () => props.displacementMapSrc || lg.displacementMapUrl.value,
);

// 容器样式
const containerStyle = computed<Record<string, string>>(() => ({
  width: props.width || "",
  height: props.height || "",
  borderRadius: `${props.cornerRadius}px`,
  padding: props.padding || "",
  transform: lg.transformStyle.value,
  boxShadow: lg.boxShadow.value,
  transition: "transform 0.2s ease-out",
}));

// warp 层样式：backdrop-filter 包含 blur + saturate + url(#id)
// url(#id) 必须在 backdrop-filter 里，让 SVG 滤镜作用于 backdrop（玻璃背后的内容）
const warpStyle = computed<Record<string, string>>(() => ({
  backdropFilter: lg.backdropFilter.value,
  WebkitBackdropFilter: lg.backdropFilter.value,
}));

// 边框层样式（screen 混合）
const borderScreenStyle = computed<Record<string, string>>(() => ({
  borderRadius: `${props.cornerRadius}px`,
  mixBlendMode: "screen",
  opacity: "0.2",
  padding: "1.5px",
  WebkitMask: "linear-gradient(#000 0 0) content-box, linear-gradient(#000 0 0)",
  WebkitMaskComposite: "xor",
  maskComposite: "exclude",
  boxShadow:
    "0 0 0 0.5px rgba(255,255,255,0.5) inset, 0 1px 3px rgba(255,255,255,0.25) inset, 0 1px 4px rgba(0,0,0,0.35)",
  background: lg.borderGradient.value,
}));

// 边框层样式（overlay 混合）
const borderOverlayStyle = computed<Record<string, string>>(() => {
  const offset = lg.mouseOffset.value;
  const angle = 135 + offset.x * 1.2;
  const midStop = Math.max(10, 33 + offset.y * 0.3);
  const endStop = Math.min(90, 66 + offset.y * 0.4);
  return {
    borderRadius: `${props.cornerRadius}px`,
    mixBlendMode: "overlay",
    opacity: "1",
    padding: "1.5px",
    WebkitMask: "linear-gradient(#000 0 0) content-box, linear-gradient(#000 0 0)",
    WebkitMaskComposite: "xor",
    maskComposite: "exclude",
    background: `linear-gradient(${angle}deg,
      rgba(255,255,255,0.0) 0%,
      rgba(255,255,255,${0.32 + Math.abs(offset.x) * 0.008}) ${midStop}%,
      rgba(255,255,255,${0.6 + Math.abs(offset.x) * 0.012}) ${endStop}%,
      rgba(255,255,255,0.0) 100%)`,
  };
});

// hover 高光层透明度
const hoverHighlightOpacity = computed(() => {
  if (lg.isActive.value) return "0.8";
  if (lg.isHovered.value) return "0.4";
  return "0";
});
const hoverSoftOpacity = computed(() => {
  if (lg.isActive.value) return "0.5";
  if (lg.isHovered.value) return "0.5";
  return "0";
});
const hoverStrongOpacity = computed(() => (lg.isActive.value ? "0.5" : "0"));

// overLight 黑色 overlay
const overLightOverlayOpacity = computed(() => (props.overLight ? "0.2" : "0"));
const overLightBlendOpacity = computed(() => (props.overLight ? "1" : "0"));

// 绑定 ref 到 composable
const setRootRef = (el: Element | ComponentPublicInstance | null) => {
  lg.glassRef.value = (el as HTMLElement) || null;
};

// 是否可点击：父组件通过 @click 监听或 tag === 'button'
const attrs = useAttrs();
const clickable = computed(
  () => props.tag === "button" || typeof attrs.onClick === "function",
);

const onClick = (ev: MouseEvent) => {
  emit("click", ev);
};
</script>

<template>
  <component
    :is="tag"
    :ref="setRootRef"
    class="liquid-glass"
    :class="{ 'is-clickable': clickable }"
    :style="containerStyle"
    @mouseenter="lg.eventHandlers.onMouseEnter"
    @mouseleave="lg.eventHandlers.onMouseLeave"
    @mousedown="lg.eventHandlers.onMouseDown"
    @mouseup="lg.eventHandlers.onMouseUp"
    @click="onClick"
  >
    <!-- warp 层：被 SVG filter 扭曲的 backdrop-filter -->
    <span class="liquid-glass__warp" aria-hidden="true" :style="warpStyle" />

    <!-- overLight 黑色降亮 overlay -->
    <div
      v-if="overLight"
      class="liquid-glass__overlay liquid-glass__overlay--dim"
      aria-hidden="true"
      :style="{ opacity: overLightOverlayOpacity }"
    />
    <div
      v-if="overLight"
      class="liquid-glass__overlay liquid-glass__overlay--blend"
      aria-hidden="true"
      :style="{ opacity: overLightBlendOpacity }"
    />

    <!-- 边框层 1：screen 混合 -->
    <span
      v-if="!borderless"
      class="liquid-glass__border"
      aria-hidden="true"
      :style="borderScreenStyle"
    />
    <!-- 边框层 2：overlay 混合 -->
    <span
      v-if="!borderless"
      class="liquid-glass__border"
      aria-hidden="true"
      :style="borderOverlayStyle"
    />

    <!-- hover 高光层 -->
    <template v-if="!noHoverHighlight">
      <div
        class="liquid-glass__highlight liquid-glass__highlight--soft"
        aria-hidden="true"
        :style="{ opacity: hoverSoftOpacity }"
      />
      <div
        class="liquid-glass__highlight liquid-glass__highlight--strong"
        aria-hidden="true"
        :style="{ opacity: hoverStrongOpacity }"
      />
      <div
        class="liquid-glass__highlight liquid-glass__highlight--overall"
        aria-hidden="true"
        :style="{ opacity: hoverHighlightOpacity }"
      />
    </template>

    <!-- 内容层 -->
    <div class="liquid-glass__content">
      <slot />
    </div>
  </component>

  <!-- SVG filter 定义（liquid-glass-react 完整 RGB 色散实现） -->
  <svg class="liquid-glass-defs" aria-hidden="true" focusable="false">
    <defs>
      <radialGradient :id="`${lg.filterId}-edge-mask`" cx="50%" cy="50%" r="50%">
        <stop offset="0%" stop-color="black" stop-opacity="0" />
        <stop :offset="`${lg.edgeMaskMiddleOffset.value}%`" stop-color="black" stop-opacity="0" />
        <stop offset="100%" stop-color="white" stop-opacity="1" />
      </radialGradient>
      <filter
        :id="lg.filterId"
        x="-35%"
        y="-35%"
        width="170%"
        height="170%"
        color-interpolation-filters="sRGB"
      >
        <!-- 1. 载入位移贴图 -->
        <feImage
          :href="displacementMapUrl"
          :xlink:href="displacementMapUrl"
          x="0"
          y="0"
          width="100%"
          height="100%"
          result="DISPLACEMENT_MAP"
          preserveAspectRatio="xMidYMid slice"
        />
        <!-- 2. 灰度化得到边缘强度 -->
        <feColorMatrix
          in="DISPLACEMENT_MAP"
          type="matrix"
          values="0.3 0.3 0.3 0 0
                  0.3 0.3 0.3 0 0
                  0.3 0.3 0.3 0 0
                  0 0 0 1 0"
          result="EDGE_INTENSITY"
        />
        <!-- 3. 阈值化得到边缘 mask -->
        <feComponentTransfer in="EDGE_INTENSITY" result="EDGE_MASK">
          <feFuncA type="discrete" :tableValues="lg.edgeMaskTableValues.value" />
        </feComponentTransfer>
        <!-- 4. 原图副本（中心干净区） -->
        <feOffset in="SourceGraphic" dx="0" dy="0" result="CENTER_ORIGINAL" />
        <!-- 5. 红通道位移 -->
        <feDisplacementMap
          in="SourceGraphic"
          in2="DISPLACEMENT_MAP"
          :scale="lg.redScale.value"
          xChannelSelector="R"
          yChannelSelector="B"
          result="RED_DISPLACED"
        />
        <feColorMatrix
          in="RED_DISPLACED"
          type="matrix"
          values="1 0 0 0 0
                  0 0 0 0 0
                  0 0 0 0 0
                  0 0 0 1 0"
          result="RED_CHANNEL"
        />
        <!-- 6. 绿通道位移（scale 递减） -->
        <feDisplacementMap
          in="SourceGraphic"
          in2="DISPLACEMENT_MAP"
          :scale="lg.greenScale.value"
          xChannelSelector="R"
          yChannelSelector="B"
          result="GREEN_DISPLACED"
        />
        <feColorMatrix
          in="GREEN_DISPLACED"
          type="matrix"
          values="0 0 0 0 0
                  0 1 0 0 0
                  0 0 0 0 0
                  0 0 0 1 0"
          result="GREEN_CHANNEL"
        />
        <!-- 7. 蓝通道位移（scale 减得最多，制造色散） -->
        <feDisplacementMap
          in="SourceGraphic"
          in2="DISPLACEMENT_MAP"
          :scale="lg.blueScale.value"
          xChannelSelector="R"
          yChannelSelector="B"
          result="BLUE_DISPLACED"
        />
        <feColorMatrix
          in="BLUE_DISPLACED"
          type="matrix"
          values="0 0 0 0 0
                  0 0 0 0 0
                  0 0 1 0 0
                  0 0 0 1 0"
          result="BLUE_CHANNEL"
        />
        <!-- 8. screen 混合三通道 -->
        <feBlend in="GREEN_CHANNEL" in2="BLUE_CHANNEL" mode="screen" result="GB_COMBINED" />
        <feBlend in="RED_CHANNEL" in2="GB_COMBINED" mode="screen" result="RGB_COMBINED" />
        <!-- 9. 轻微高斯模糊柔化色散 -->
        <feGaussianBlur in="RGB_COMBINED" :stdDeviation="lg.aberrationBlur.value" result="ABERRATED_BLURRED" />
        <!-- 10. 边缘 mask 限定色散区域 -->
        <feComposite in="ABERRATED_BLURRED" in2="EDGE_MASK" operator="in" result="EDGE_ABERRATION" />
        <!-- 11. 反转 mask，得到中心干净区 -->
        <feComponentTransfer in="EDGE_MASK" result="INVERTED_MASK">
          <feFuncA type="table" tableValues="1 0" />
        </feComponentTransfer>
        <feComposite in="CENTER_ORIGINAL" in2="INVERTED_MASK" operator="in" result="CENTER_CLEAN" />
        <!-- 12. 边缘色散 + 中心干净图叠加 -->
        <feComposite in="EDGE_ABERRATION" in2="CENTER_CLEAN" operator="over" />
      </filter>
    </defs>
  </svg>
</template>

<style scoped>
.liquid-glass {
  position: relative;
  display: inline-flex;
  align-items: center;
  gap: 24px;
  overflow: hidden;
  /* button 元素重置 */
  appearance: none;
  -webkit-appearance: none;
  margin: 0;
  font: inherit;
  color: inherit;
  background: transparent;
  border: none;
  cursor: default;
  outline: none;
  box-sizing: border-box;
  isolation: isolate;
}

.liquid-glass.is-clickable {
  cursor: pointer;
}

.liquid-glass:focus-visible {
  outline: 2px solid rgba(0, 122, 255, 0.6);
  outline-offset: 2px;
}

/* warp 层：被 SVG filter 扭曲的 backdrop */
.liquid-glass__warp {
  position: absolute;
  inset: 0;
  border-radius: inherit;
  pointer-events: none;
  z-index: 0;
  /* GPU: 独立合成层 */
  will-change: backdrop-filter;
  contain: paint;
}

/* overLight overlay */
.liquid-glass__overlay {
  position: absolute;
  inset: 0;
  border-radius: inherit;
  pointer-events: none;
  z-index: 0;
  transition: opacity 0.2s;
}

.liquid-glass__overlay--dim {
  background: black;
}

.liquid-glass__overlay--blend {
  background: black;
  mix-blend-mode: overlay;
}

/* 边框层 */
.liquid-glass__border {
  position: absolute;
  inset: 0;
  pointer-events: none;
  z-index: 1;
  transition: background 0.2s;
}

/* hover 高光层 */
.liquid-glass__highlight {
  position: absolute;
  inset: 0;
  border-radius: inherit;
  pointer-events: none;
  z-index: 2;
  mix-blend-mode: overlay;
  transition: opacity 0.2s;
}

.liquid-glass__highlight--soft {
  background-image: radial-gradient(
    circle at 50% 0%,
    rgba(255, 255, 255, 0.5) 0%,
    rgba(255, 255, 255, 0) 50%
  );
}

.liquid-glass__highlight--strong {
  background-image: radial-gradient(
    circle at 50% 0%,
    rgba(255, 255, 255, 1) 0%,
    rgba(255, 255, 255, 0) 80%
  );
}

.liquid-glass__highlight--overall {
  background-image: radial-gradient(
    circle at 50% 0%,
    rgba(255, 255, 255, 1) 0%,
    rgba(255, 255, 255, 0) 100%
  );
}

/* 内容层 */
.liquid-glass__content {
  position: relative;
  z-index: 3;
  text-shadow: 0px 2px 12px rgba(0, 0, 0, 0.4);
  color: white;
  font-weight: 500;
}

/* SVG defs：仅承载 filter 定义，不占布局空间 */
.liquid-glass-defs {
  position: absolute;
  width: 0;
  height: 0;
  pointer-events: none;
  overflow: hidden;
}

@media (prefers-color-scheme: dark) {
  .liquid-glass__content {
    text-shadow: 0px 2px 12px rgba(0, 0, 0, 0.55);
  }
}

@media (prefers-reduced-motion: reduce) {
  .liquid-glass {
    transition: none !important;
  }
  .liquid-glass__highlight,
  .liquid-glass__overlay {
    transition: none !important;
  }
}
</style>
