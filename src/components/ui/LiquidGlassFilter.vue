<script setup>
/**
 * LiquidGlassFilter - 液态玻璃 SVG filter 渲染组件
 *
 * 配合 `useLiquidGlass` composable 使用：
 * composable 负责状态与样式，本组件负责渲染 SVG `<defs><filter>` 节点。
 *
 * 滤镜链路（移植自 liquid-glass-react + AppBottomNav 既有实现）：
 *   feImage（位移图）
 *     → feGaussianBlur（平滑高频噪声）
 *     → feColorMatrix（压低对比度，中心保持清晰）
 *     → [可选] feTurbulence + feBlend（叠加噪声流动感）
 *     → feDisplacementMap（对 SourceGraphic 做边缘折射）
 *
 * 注意：本组件不渲染视觉内容，仅承载 filter 定义，因此根 svg 元素
 * 设置 width/height:0 + position:absolute，不占布局空间。
 *
 * 通过 `registerDisplacementMap` 回调将 `<feDisplacementMap>` 元素注册回
 * composable，使 hover 时 composable 可以驱动 scale attribute 动画。
 */
import { onMounted, onBeforeUnmount, useTemplateRef, watch } from 'vue';

const props = defineProps({
  /** 唯一 filter id（由 useLiquidGlass 通过 useId 生成） */
  filterId: { type: String, required: true },
  /** 位移映射图 URL（data URI 或远程 URL） */
  displacementMapUrl: { type: String, required: true },
  /** 位移强度（objectBoundingBox 倍数） */
  scale: { type: Number, default: 0.015 },
  /** 位移图平滑核 */
  mapBlur: { type: Number, default: 0.03 },
  /** 噪声频率（0 = 关闭） */
  turbulence: { type: Number, default: 0 },
  /**
   * 注册 <feDisplacementMap> 元素回 composable 的回调。
   * composable 通过此回调获得元素引用，以便 hover 时动画 scale attribute。
   */
  registerDisplacementMap: { type: Function, default: null },
});

const dispMapEl = useTemplateRef('dispMapEl');

// 同步 scale attribute：Vue 对 SVG filter primitive attribute 绑定不稳定，
// 通过 watch 显式 setAttribute 保证响应式更新
watch(() => props.scale, (v) => {
  if (dispMapEl.value) dispMapEl.value.setAttribute('scale', String(v));
});

// 挂载时将 feDisplacementMap 元素注册回 composable
onMounted(() => {
  if (props.registerDisplacementMap && dispMapEl.value) {
    props.registerDisplacementMap(dispMapEl.value);
  }
});

// 卸载时清除注册，避免 composable 持有失效引用
onBeforeUnmount(() => {
  if (props.registerDisplacementMap) {
    props.registerDisplacementMap(null);
  }
});
</script>

<template>
  <svg class="liquid-glass-defs" aria-hidden="true" focusable="false">
    <filter
      :id="filterId"
      primitiveUnits="objectBoundingBox"
      x="-10%"
      y="-10%"
      width="120%"
      height="120%"
    >
      <feImage
        :href="displacementMapUrl"
        :xlink:href="displacementMapUrl"
        x="0"
        y="0"
        width="1"
        height="1"
        result="map"
        preserveAspectRatio="none"
      />
      <!-- 平滑位移图：降低高频噪声，避免局部过度拉伸 -->
      <feGaussianBlur in="map" :stdDeviation="mapBlur" result="smoothMap" />
      <!-- 压低对比度：中心保持清晰，仅边缘产生极轻微折射 -->
      <feColorMatrix
        in="smoothMap"
        type="matrix"
        values="0.18 0 0 0 0.41
                0 0.18 0 0 0.41
                0 0 0.18 0 0.41
                0 0 0 1 0"
        result="tonedMap"
      />
      <feTurbulence
        v-if="turbulence > 0"
        type="fractalNoise"
        :baseFrequency="turbulence"
        numOctaves="2"
        seed="7"
        result="noise"
      />
      <feBlend
        v-if="turbulence > 0"
        in="tonedMap"
        in2="noise"
        mode="overlay"
        result="combinedMap"
      />
      <!--
        CSS backdrop-filter: blur() 负责各向同性磨砂模糊，
        SVG filter 只负责轻微边缘折射。
      -->
      <feDisplacementMap
        ref="dispMapEl"
        in="SourceGraphic"
        :in2="turbulence > 0 ? 'combinedMap' : 'tonedMap'"
        :scale="scale"
        xChannelSelector="R"
        yChannelSelector="G"
      />
    </filter>
  </svg>
</template>

<style scoped>
.liquid-glass-defs {
  position: absolute;
  width: 0;
  height: 0;
  pointer-events: none;
  overflow: hidden;
}
</style>
