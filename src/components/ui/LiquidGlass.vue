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
 * - 透传所有库组件 props 和 onClick 回调
 *
 * 库组件内部管理：
 * - SVG filter 定义和生命周期（挂载/卸载自动清理）
 * - elasticity 鼠标跟随（默认响应玻璃自身范围；如需响应外部容器，
 *   通过 mouseContainer 传入 HTMLElement，注意容器必须能接收 mousemove）
 * - hover/click 交互效果
 *
 * 注意：
 * - 库组件不 emit `click` 事件，而是通过 `onClick` prop 回调处理点击。
 *   本包装组件接收 `onClick` prop 并透传，父组件用 `@click="handler"` 即可
 *   （无 emits 声明时，Vue 会自动把 `@click` 解释为 `onClick` prop）。
 * - 不要把 `pointer-events:none` 的元素作为 mouseContainer，它无法触发
 *   mousemove 事件，且会触发库组件内部 watchEffect 的递归更新。
 *
 * 用法：
 * ```vue
 * <LiquidGlass :corner-radius="32" padding="0 16px" :elasticity="0.2">
 *   <nav>...</nav>
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
    /**
     * 鼠标容器 HTMLElement（用于父容器响应鼠标）
     * 注意：必须是能接收 mousemove 事件的元素，不能是 pointer-events:none
     */
    mouseContainer?: HTMLElement | null;
    /** 全局鼠标位置（手动控制） */
    globalMousePos?: { x: number; y: number };
    /** 鼠标位置偏移 */
    mouseOffset?: { x: number; y: number };
    /** 点击回调（库组件通过 onClick prop 处理点击，非 emit click 事件） */
    onClick?: () => void;
    /** 额外 CSS 类（避免与 Vue 保留的 class 属性冲突） */
    extraClass?: string;
    /** 额外内联样式 */
    style?: Record<string, string>;
  }>(),
  {
    ...LIQUID_GLASS_PRESETS.user,
    cornerRadius: 999,
    padding: "0px",
  },
);

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

// 合并用户 extraClass 与项目默认 class
const mergedClass = computed(() =>
  ["chordial-liquid-glass", props.extraClass].filter(Boolean).join(" "),
);
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
    :on-click="onClick"
    :class="mergedClass"
    :style="mergedStyle"
  >
    <slot />
  </LibLiquidGlass>
</template>

<style>
/* 项目主题样式：深色模式适配
   注意：库组件返回 Fragment（多根节点），scoped 样式无法自动应用，
   因此使用非 scoped 全局样式 + .chordial-liquid-glass 精确选择器 */
.chordial-liquid-glass {
  @media (prefers-color-scheme: dark) {
    background: rgba(40, 40, 45, 0.5) !important;
    border-color: rgba(255, 255, 255, 0.15) !important;
    box-shadow:
      0 8px 24px rgba(0, 0, 0, 0.35),
      inset 0 1px 0 rgba(255, 255, 255, 0.1) !important;
  }
}
</style>
