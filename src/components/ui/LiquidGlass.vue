<script setup lang="ts">
/**
 * LiquidGlass - 项目统一的液态玻璃包装组件
 *
 * 基于 liquid-glass-react（https://github.com/rdev/liquid-glass-react）算法，
 * 采用 Vue 封装 React 库的方式实现：
 * - React 渲染的液态玻璃层仅作为 background，position: absolute，z-index: 0
 * - Vue slot 内容平铺在 z-index: 1 的内容层，不会 Teleport 到 React 子容器中
 * - 保留与原组件一致的 props 接口（displacementScale、blurAmount、saturation、
 *   aberrationIntensity、elasticity、cornerRadius、mode、padding 等）
 *
 * 结构示例：
 * <div class="chordial-liquid-glass">
 *   <div class="chordial-liquid-glass__react">  <!-- React 背景层 --></div>
 *   <div class="chordial-liquid-glass__content"><slot /></div>
 * </div>
 */
import {
  computed,
  onBeforeUnmount,
  onMounted,
  ref,
  watch,
} from "vue";
import { createElement } from "react";
import { createRoot } from "react-dom/client";
import LiquidGlassReact from "@/lib/liquid-glass-react/index.tsx";

const props = withDefaults(
  defineProps<{
    /** 位移量表（控制边缘变形强度） */
    displacementScale?: number;
    /** 模糊量（0-1，控制背景模糊强度） */
    blurAmount?: number;
    /** 饱和度百分比（控制背景色彩饱和度） */
    saturation?: number;
    /** 像差强度（控制 RGB 通道分离强度） */
    aberrationIntensity?: number;
    /** 弹性（0-1，控制玻璃向光标延伸的程度） */
    elasticity?: number;
    /** 圆角（像素值），默认 100px */
    cornerRadius?: number;
    /** 折射模式 */
    mode?: "standard" | "polar" | "prominent" | "shader";
    /** 是否在浅色背景上 */
    overLight?: boolean;
    /** CSS 内边距 */
    padding?: string;
    /** 点击回调 */
    onClick?: () => void;
    /** 额外 CSS 类 */
    extraClass?: string;
    /** 额外内联样式 */
    style?: Record<string, string>;
  }>(),
  {
    displacementScale: 150,
    blurAmount: 0.1,
    saturation: 130,
    aberrationIntensity: 2,
    elasticity: 0.35,
    cornerRadius: 100,
    mode: "standard",
    overLight: false,
    padding: "0px",
  },
);

const emit = defineEmits<{
  click: [];
}>();

const wrapperRef = ref<HTMLElement | null>(null);
const reactHostRef = ref<HTMLDivElement | null>(null);
let root: ReturnType<typeof createRoot> | null = null;

// React 期望的 ref 对象，指向鼠标容器（wrapper 根节点）
const mouseContainerReactRef = { current: null as HTMLElement | null };

const wrapperStyle = computed(() => ({
  position: "relative" as const,
  ...props.style,
}));

const contentStyle = computed(() => ({
  padding: props.padding,
}));

function handleClick() {
  props.onClick?.();
  emit("click");
}

function renderReact() {
  if (!root) return;

  mouseContainerReactRef.current = wrapperRef.value;

  const reactStyle = {
    position: "absolute" as const,
    width: "100%",
    height: "100%",
    top: "0px",
    left: "0px",
    right: "0px",
    bottom: "0px",
    transform: "none",
  };

  root.render(
    createElement(
      LiquidGlassReact,
      {
        displacementScale: props.displacementScale,
        blurAmount: props.blurAmount,
        saturation: props.saturation,
        aberrationIntensity: props.aberrationIntensity,
        elasticity: props.elasticity,
        cornerRadius: props.cornerRadius,
        padding: "0",
        overLight: props.overLight,
        mode: props.mode,
        style: reactStyle,
        mouseContainer: mouseContainerReactRef,
      },
      // React 子节点仅作为尺寸占位，真正的 Vue slot 内容在内容层渲染
      createElement("div", { style: { width: "100%", height: "100%" } }),
    ),
  );
}

onMounted(() => {
  if (reactHostRef.value) {
    root = createRoot(reactHostRef.value);
    renderReact();
  }
});

onBeforeUnmount(() => {
  root?.unmount();
  root = null;
});

watch(
  () => [
    props.displacementScale,
    props.blurAmount,
    props.saturation,
    props.aberrationIntensity,
    props.elasticity,
    props.cornerRadius,
    props.mode,
    props.overLight,
    props.padding,
    props.extraClass,
    props.style,
    wrapperRef.value,
  ],
  renderReact,
  { deep: true },
);
</script>

<template>
  <div
    ref="wrapperRef"
    class="chordial-liquid-glass"
    :class="extraClass"
    :style="wrapperStyle"
  >
    <div ref="reactHostRef" class="chordial-liquid-glass__react" />
    <div
      class="chordial-liquid-glass__content"
      :style="contentStyle"
      @click="handleClick"
    >
      <slot />
    </div>
  </div>
</template>

<style>
/* 项目主题：半透明乳白磨砂玻璃底色 */
.chordial-liquid-glass {
  position: relative;
  isolation: isolate;
}

.chordial-liquid-glass__react {
  position: absolute;
  inset: 0;
  z-index: 0;
  pointer-events: none;
}

.chordial-liquid-glass__content {
  position: relative;
  z-index: 1;
  display: flex;
  align-items: center;
  width: 100%;
}

/* React 库内部 .glass 默认 inline-flex 且随内容缩放；
   背景层需要充满容器，因此强制 block + 100% 宽高 */
.chordial-liquid-glass__react .glass {
  display: block !important;
  width: 100% !important;
  height: 100% !important;
}

.chordial-liquid-glass__react .glass__warp {
  border-radius: inherit !important;
}

/* liquid-glass-react 内部使用 Tailwind 工具类，
   项目未引入 Tailwind，因此提供最小等价定义以保证视觉效果 */
.chordial-liquid-glass__react .relative {
  position: relative;
}

.chordial-liquid-glass__react .transition-all {
  transition-property: all;
  transition-timing-function: cubic-bezier(0.4, 0, 0.2, 1);
  transition-duration: 150ms;
}

.chordial-liquid-glass__react .duration-150 {
  transition-duration: 150ms;
}

.chordial-liquid-glass__react .ease-in-out {
  transition-timing-function: cubic-bezier(0.4, 0, 0.2, 1);
}

.chordial-liquid-glass__react .text-white {
  color: #ffffff;
}

.chordial-liquid-glass__react .pointer-events-none {
  pointer-events: none;
}

.chordial-liquid-glass__react .bg-black {
  background-color: #000000;
}

.chordial-liquid-glass__react .mix-blend-overlay {
  mix-blend-mode: overlay;
}

.chordial-liquid-glass__react .opacity-0 {
  opacity: 0;
}

.chordial-liquid-glass__react .opacity-20 {
  opacity: 0.2;
}

.chordial-liquid-glass__react .opacity-100 {
  opacity: 1;
}

.chordial-liquid-glass__react .cursor-pointer {
  cursor: pointer;
}

@media (prefers-color-scheme: dark) {
  .chordial-liquid-glass {
    /* 深色模式下由 React 层自行处理 overLight/shadow，
       内容层不需要额外背景覆盖 */
  }
}
</style>
