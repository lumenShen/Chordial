<script lang="ts">
/**
 * 用户预设参数（与 useLiquidGlass 时代的 LIQUID_GLASS_PRESETS.user 保持一致）
 */
export const LIQUID_GLASS_PRESETS = {
  user: {
    displacementScale: 150,
    blurAmount: 0.1,
    saturation: 130,
    aberrationIntensity: 2,
    elasticity: 0.35,
    mode: "standard" as const,
  },
} as const;
</script>

<script setup lang="ts">
/**
 * LiquidGlass - 项目统一的液态玻璃包装组件
 *
 * 基于 @wxperia/liquid-glass-vue 的 <LiquidGlass> 组件封装：
 * - 内置用户预设参数（折射模式=标准, 位移量表=150, 模糊量=0.1, 饱和度=130%,
 *   像差强度=2, 弹性=0.35）
 * - 注入项目主题色背景（浅色/深色模式自动适配）
 * - 透传所有库组件 props 和事件，支持覆盖默认预设
 *
 * 库组件内部管理：
 * - SVG filter 定义和生命周期（挂载/卸载自动清理）
 * - elasticity 鼠标跟随（通过 mouseContainer / globalMousePos 控制）
 * - hover/click 交互效果
 *
 * 用法：
 * ```vue
 * <LiquidGlass :corner-radius="32" padding="0 16px" :elasticity="0.2">
 *   <nav>...</nav>
 * </LiquidGlass>
 * ```
 *
 * 对于需要 mouseContainer 的场景（如 AppHeader 顶部 drag region 作为鼠标响应区）：
 * ```vue
 * <LiquidGlass :mouse-container="dragRegionRef" :elasticity="0.35">
 *   <button>...</button>
 * </LiquidGlass>
 * ```
 */
import { computed } from "vue";
import { LiquidGlass as LibLiquidGlass } from "@wxperia/liquid-glass-vue";

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
    /** 圆角（像素值） */
    cornerRadius?: number;
    /** 折射模式 */
    mode?: "standard" | "polar" | "prominent" | "shader";
    /** 着色器效果类型（仅 mode='shader' 时有效） */
    effect?: "flowingLiquid" | "liquidGlass" | "transparentIce" | "unevenGlass" | "mosaicGlass";
    /** 是否在浅色背景上 */
    overLight?: boolean;
    /** CSS 内边距 */
    padding?: string;
    /** 鼠标容器 ref（用于父容器响应鼠标） */
    mouseContainer?: ReturnType<typeof import("vue").ref<HTMLElement | null>>;
    /** 全局鼠标位置（手动控制） */
    globalMousePos?: { x: number; y: number };
    /** 鼠标位置偏移 */
    mouseOffset?: { x: number; y: number };
    /** 额外 CSS 类 */
    class?: string;
    /** 额外内联样式 */
    style?: Record<string, string>;
  }>(),
  {
    ...LIQUID_GLASS_PRESETS.user,
    cornerRadius: 999,
    padding: "0px",
  },
);

defineEmits<{
  click: [];
}>();

// 合并用户 style 与项目主题色背景
const mergedStyle = computed<Record<string, string>>(() => ({
  // 项目主题：半透明乳白磨砂玻璃底色
  // 库组件本身不提供背景色，需通过 style 注入
  background: "rgba(255, 255, 255, 0.22)",
  border: "1px solid rgba(255, 255, 255, 0.35)",
  boxShadow:
    "0 8px 24px rgba(0, 0, 0, 0.08), inset 0 1px 0 rgba(255, 255, 255, 0.5)",
  ...props.style,
}));

// 合并用户 class 与项目默认 class
const mergedClass = computed(() => ["chordial-liquid-glass", props.class].filter(Boolean).join(" "));
</script>

<template>
  <LibLiquidGlass
    :displacement-scale="displacementScale"
    :blur-amount="blurAmount"
    :saturation="saturation"
    :aberration-intensity="aberrationIntensity"
    :elasticity="elasticity"
    :corner-radius="cornerRadius"
    :mode="mode"
    :effect="effect"
    :over-light="overLight"
    :padding="padding"
    :mouse-container="mouseContainer"
    :global-mouse-pos="globalMousePos"
    :mouse-offset="mouseOffset"
    :class="mergedClass"
    :style="mergedStyle"
    @click="$emit('click')"
  >
    <slot />
  </LibLiquidGlass>
</template>

<style scoped>
.chordial-liquid-glass {
  /* 深色模式：深灰玻璃底色 */
  @media (prefers-color-scheme: dark) {
    background: rgba(40, 40, 45, 0.5) !important;
    border-color: rgba(255, 255, 255, 0.15) !important;
    box-shadow:
      0 8px 24px rgba(0, 0, 0, 0.35),
      inset 0 1px 0 rgba(255, 255, 255, 0.1) !important;
  }
}
</style>
