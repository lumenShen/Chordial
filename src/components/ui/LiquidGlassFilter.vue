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
 *     → feDisplacementMap（对 SourceGraphic 做边缘折射）
 *
 * 像差模式（aberration > 0）：
 *   对 SourceGraphic 分离 R/G/B 三个通道，分别用不同 scale 位移：
 *     R 通道：scale + aberration
 *     G 通道：scale
 *     B 通道：scale - aberration
 *   再用 feBlend screen 合成，产生色差（chromatic aberration）效果。
 *
 * 注意：本组件不渲染视觉内容，仅承载 filter 定义，因此根 svg 元素
 * 设置 width/height:0 + position:absolute，不占布局空间。
 *
 * 通过 `registerDisplacementMap` 回调将所有 `<feDisplacementMap>` 元素注册回
 * composable，使 hover 时 composable 可以驱动 scale attribute 动画。
 */
import { computed, nextTick, onMounted, onBeforeUnmount, useTemplateRef, watch } from 'vue';

const props = defineProps({
  /** 唯一 filter id（由 useLiquidGlass 通过 useId 生成） */
  filterId: { type: String, required: true },
  /** 位移映射图 URL（data URI 或远程 URL） */
  displacementMapUrl: { type: String, required: true },
  /** 位移强度（objectBoundingBox 倍数，G 通道基准） */
  scale: { type: Number, default: 0.015 },
  /** 像差强度（像素值，控制 RGB 通道分离强度，0 = 关闭） */
  aberration: { type: Number, default: 0 },
  /** 位移图平滑核 */
  mapBlur: { type: Number, default: 0.03 },
  /** 噪声频率（0 = 关闭） */
  turbulence: { type: Number, default: 0 },
  /**
   * 注册 <feDisplacementMap> 元素回 composable 的回调。
   * composable 通过此回调获得元素引用，以便 hover 时动画 scale attribute。
   * 支持传入元素数组（aberration 模式）或 null（清空）。
   */
  registerDisplacementMap: { type: Function, default: null },
});

// 三个 feDisplacementMap 元素引用（aberration 模式下 R/G/B 通道；非 aberration 模式仅 G 通道）
const dispMapREl = useTemplateRef('dispMapR');
const dispMapGEl = useTemplateRef('dispMapG');
const dispMapBEl = useTemplateRef('dispMapB');

/**
 * aberration 转换为 objectBoundingBox 倍数。
 * aberration 是像素强度（0-10 范围），/ 100 转为 obb 倍数：
 * 2 → 0.02，对 60px 元素约 1.2px 通道分离
 */
const aberrationObb = computed(() => props.aberration / 100);

/** R 通道 scale：基准 + 像差 */
const scaleR = computed(() => props.scale + aberrationObb.value);
/** G 通道 scale：基准 */
const scaleG = computed(() => props.scale);
/** B 通道 scale：基准 - 像差 */
const scaleB = computed(() => Math.max(0, props.scale - aberrationObb.value));

/** 是否启用像差模式 */
const hasAberration = computed(() => props.aberration > 0);

/**
 * 收集当前所有已渲染的 feDisplacementMap 元素。
 * aberration 模式：3 个；非 aberration 模式：1 个（G 通道）
 */
function collectDispMapEls() {
  if (hasAberration.value) {
    return [dispMapREl.value, dispMapGEl.value, dispMapBEl.value].filter(Boolean);
  }
  return dispMapGEl.value ? [dispMapGEl.value] : [];
}

/**
 * 同步所有 feDisplacementMap 的 scale attribute。
 * Vue 对 SVG filter primitive attribute 绑定不稳定，通过显式 setAttribute 保证更新。
 */
function syncScaleAttributes() {
  if (hasAberration.value) {
    dispMapREl.value?.setAttribute('scale', String(scaleR.value));
    dispMapGEl.value?.setAttribute('scale', String(scaleG.value));
    dispMapBEl.value?.setAttribute('scale', String(scaleB.value));
  } else {
    dispMapGEl.value?.setAttribute('scale', String(scaleG.value));
  }
}

/**
 * 重新注册 feDisplacementMap 元素到 composable。
 * 在 aberration 模式切换时调用（v-if 会导致元素创建/销毁）。
 */
function reregister() {
  if (!props.registerDisplacementMap) return;
  nextTick(() => {
    const els = collectDispMapEls();
    props.registerDisplacementMap(els);
    syncScaleAttributes();
  });
}

// scale 变化时同步所有 feDisplacementMap attribute
watch(
  () => props.scale,
  () => syncScaleAttributes(),
);

// aberration 变化时：模式可能切换，需重新注册元素
watch(
  () => props.aberration,
  () => reregister(),
);

// 挂载时将 feDisplacementMap 元素注册回 composable
onMounted(() => {
  reregister();
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
      x="-20%"
      y="-20%"
      width="140%"
      height="140%"
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
        位移图最终结果：turbulence > 0 时用 combinedMap，否则用 tonedMap
      -->
      <feColorMatrix
        v-if="turbulence > 0"
        in="combinedMap"
        type="matrix"
        values="1 0 0 0 0
                0 1 0 0 0
                0 0 1 0 0
                0 0 0 1 0"
        result="finalMap"
      />
      <feColorMatrix
        v-else
        in="tonedMap"
        type="matrix"
        values="1 0 0 0 0
                0 1 0 0 0
                0 0 1 0 0
                0 0 0 1 0"
        result="finalMap"
      />

      <!-- ── 像差模式（aberration > 0）：RGB 通道分离位移 ── -->
      <template v-if="hasAberration">
        <!-- R 通道：提取 R（清零 G/B），用 scale + aberration 位移 -->
        <feColorMatrix
          in="SourceGraphic"
          type="matrix"
          values="1 0 0 0 0
                  0 0 0 0 0
                  0 0 0 0 0
                  0 0 0 1 0"
          result="rSrc"
        />
        <feDisplacementMap
          ref="dispMapR"
          in="rSrc"
          in2="finalMap"
          :scale="scaleR"
          xChannelSelector="R"
          yChannelSelector="G"
          result="rDisp"
        />
        <!-- G 通道：提取 G，用 scale 位移 -->
        <feColorMatrix
          in="SourceGraphic"
          type="matrix"
          values="0 0 0 0 0
                  0 1 0 0 0
                  0 0 0 0 0
                  0 0 0 1 0"
          result="gSrc"
        />
        <feDisplacementMap
          ref="dispMapG"
          in="gSrc"
          in2="finalMap"
          :scale="scaleG"
          xChannelSelector="R"
          yChannelSelector="G"
          result="gDisp"
        />
        <!-- B 通道：提取 B，用 scale - aberration 位移 -->
        <feColorMatrix
          in="SourceGraphic"
          type="matrix"
          values="0 0 0 0 0
                  0 0 0 0 0
                  0 0 1 0 0
                  0 0 0 1 0"
          result="bSrc"
        />
        <feDisplacementMap
          ref="dispMapB"
          in="bSrc"
          in2="finalMap"
          :scale="scaleB"
          xChannelSelector="R"
          yChannelSelector="G"
          result="bDisp"
        />
        <!-- 合成：screen 模式相加 RGB 通道 -->
        <feBlend in="rDisp" in2="gDisp" mode="screen" result="rgDisp" />
        <feBlend in="rgDisp" in2="bDisp" mode="screen" result="rgbDisp" />
      </template>

      <!-- ── 非像差模式（aberration = 0）：单次位移 ── -->
      <template v-else>
        <!--
          CSS backdrop-filter: blur() 负责各向同性磨砂模糊，
          SVG filter 只负责轻微边缘折射。
        -->
        <feDisplacementMap
          ref="dispMapG"
          in="SourceGraphic"
          in2="finalMap"
          :scale="scaleG"
          xChannelSelector="R"
          yChannelSelector="G"
        />
      </template>
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
