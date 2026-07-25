<script setup>
import { computed, onMounted, useTemplateRef, watch, nextTick } from 'vue';
import { useRoute } from 'vue-router';
import { useAnime } from '@/composables/useAnime.js';
import { ANIME_SPRINGS } from '@/utils/animePresets.js';
import { useLiquidGlass } from '@/composables/useLiquidGlass.js';

const route = useRoute();

const rootRef = useTemplateRef('root');
const indicatorRef = useTemplateRef('indicator');
const { run, animate } = useAnime(() => rootRef.value);

// ===== 液态玻璃 composable =====
// 侧边栏参数：位移强度较低（侧边栏较高，避免过度扭曲），饱和度增强
const lg = useLiquidGlass({
  displacementScale: 20,
  blurAmount: 0.0625,
  saturation: 180,
  aberrationIntensity: 2,
  elasticity: 0.15,
  overLight: false,
  mode: 'standard',
  interactive: true,
});

// 菜单项错峰入场（slideInLeft + stagger）
onMounted(() => {
  run(({ animate, stagger, presets }) => {
    animate('.nav-item', {
      ...presets.slideInLeft,
      delay: stagger(50),
      onComplete: () => {
        // 清除内联 transform，恢复 CSS :hover / .active 过渡
        rootRef.value?.querySelectorAll('.nav-item').forEach((el) => {
          el.style.transform = '';
        });
      },
    });
  });
  // 初始化 active 指示器
  nextTick(updateIndicator);
  // 绑定 glassRef 到 sidebar 根元素
  lg.glassRef.value = rootRef.value;
});

const navItems = [
  {
    path: '/',
    name: '首页',
    icon: 'bi-house'
  },
  {
    path: '/search',
    name: '搜索',
    icon: 'bi-search'
  },
  {
    path: '/tracks',
    name: '歌曲',
    icon: 'bi-music-note-beamed'
  },
  {
    path: '/artists',
    name: '歌手',
    icon: 'bi-people'
  },
  {
    path: '/albums',
    name: '专辑',
    icon: 'bi-disc'
  },
  {
    path: '/playlists',
    name: '歌单',
    icon: 'bi-music-note-list'
  },
  {
    path: '/music-sources',
    name: '音乐源',
    icon: 'bi-folder'
  }
];

const isActive = (path) => {
  if (path === '/') {
    return route.path === '/';
  }
  return route.path.startsWith(path);
};

// active 指示器滑动动画（anime.js spring 物理，垂直滑动）
function updateIndicator() {
  const root = rootRef.value;
  const indicator = indicatorRef.value;
  if (!root || !indicator) return;
  const activeEl = root.querySelector('.nav-item.active');
  if (!activeEl) {
    animate(indicator, { opacity: 0, duration: 200, ease: ANIME_SPRINGS.sensitive });
    return;
  }
  const rootRect = root.getBoundingClientRect();
  const itemRect = activeEl.getBoundingClientRect();
  const y = itemRect.top - rootRect.top;
  const h = itemRect.height;
  animate(indicator, {
    translateY: y,
    height: h,
    opacity: 1,
    duration: 420,
    ease: ANIME_SPRINGS.bouncy,
  });
}

// 路由切换 → FLIP 指示器滑动
watch(() => route.path, () => {
  nextTick(updateIndicator);
});

// warp 层样式：backdrop-filter 做磨砂，filter 做 SVG 边缘色散
const warpStyle = computed(() => ({
  backdropFilter: lg.backdropFilter.value,
  WebkitBackdropFilter: lg.backdropFilter.value,
  filter: lg.svgFilter.value,
}));
</script>

<template>
  <aside ref="root" class="app-sidebar">
    <!--
      玻璃扭曲层。
      - backdrop-filter 先对 <aside> 背后的内容做磨砂/饱和
      - filter: url(#id) 再对模糊后的结果做边缘色散与折射
      - pointer-events:none 不拦截导航项点击
    -->
    <span class="glass-warp" aria-hidden="true" :style="warpStyle" />

    <div ref="indicator" class="nav-indicator"></div>
    <nav class="sidebar-nav">
      <router-link
        v-for="item in navItems"
        :key="item.path"
        :to="item.path"
        class="nav-item"
        :class="{ active: isActive(item.path) }"
      >
        <i :class="item.icon" class="nav-icon"></i>
        <span class="nav-text">{{ item.name }}</span>
      </router-link>
    </nav>

    <div class="sidebar-footer">
      <router-link to="/about" class="nav-item" :class="{ active: isActive('/about') }">
        <i class="bi bi-info-circle nav-icon"></i>
        <span class="nav-text">关于</span>
      </router-link>
    </div>
  </aside>

  <!--
    液态玻璃 SVG filter 定义（由 useLiquidGlass composable 提供参数）。
    - 完整 RGB 三通道色散实现
    - filter id 由 composable 的 useId 生成，多实例安全
  -->
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
        <feImage
          :href="lg.displacementMapUrl.value"
          :xlink:href="lg.displacementMapUrl.value"
          x="0"
          y="0"
          width="100%"
          height="100%"
          result="DISPLACEMENT_MAP"
          preserveAspectRatio="xMidYMid slice"
        />
        <feColorMatrix
          in="DISPLACEMENT_MAP"
          type="matrix"
          values="0.3 0.3 0.3 0 0
                  0.3 0.3 0.3 0 0
                  0.3 0.3 0.3 0 0
                  0 0 0 1 0"
          result="EDGE_INTENSITY"
        />
        <feComponentTransfer in="EDGE_INTENSITY" result="EDGE_MASK">
          <feFuncA type="discrete" :tableValues="lg.edgeMaskTableValues.value" />
        </feComponentTransfer>
        <feOffset in="SourceGraphic" dx="0" dy="0" result="CENTER_ORIGINAL" />
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
        <feBlend in="GREEN_CHANNEL" in2="BLUE_CHANNEL" mode="screen" result="GB_COMBINED" />
        <feBlend in="RED_CHANNEL" in2="GB_COMBINED" mode="screen" result="RGB_COMBINED" />
        <feGaussianBlur in="RGB_COMBINED" :stdDeviation="lg.aberrationBlur.value" result="ABERRATED_BLURRED" />
        <feComposite in="ABERRATED_BLURRED" in2="EDGE_MASK" operator="in" result="EDGE_ABERRATION" />
        <feComponentTransfer in="EDGE_MASK" result="INVERTED_MASK">
          <feFuncA type="table" tableValues="1 0" />
        </feComponentTransfer>
        <feComposite in="CENTER_ORIGINAL" in2="INVERTED_MASK" operator="in" result="CENTER_CLEAN" />
        <feComposite in="EDGE_ABERRATION" in2="CENTER_CLEAN" operator="over" />
      </filter>
    </defs>
  </svg>
</template>

<style scoped>
.app-sidebar {
  width: var(--sidebar-width);
  height: 100%;
  display: flex;
  flex-direction: column;
  margin: 28px 16px;
  left: 2rem;  border-radius: calc(var(--bottom-nav-height) * 0.5);
  /* 玻璃效果由 .glass-warp 层提供，此处移除 background/border */
  overflow: hidden;

  height: fit-content;
  position: relative;
}

/* 玻璃扭曲层：backdrop-filter 做磨砂，filter 做边缘色散 */
.glass-warp {
  position: absolute;
  inset: 0;
  border-radius: inherit;
  pointer-events: none;
  z-index: 0;
}

/* SVG defs：仅承载 filter 定义，不占布局空间 */
.liquid-glass-defs {
  position: absolute;
  width: 0;
  height: 0;
  pointer-events: none;
  overflow: hidden;
}

/* active 指示器（绝对定位背景，由 anime.js animate translateY/height 垂直滑动） */
.nav-indicator {
  position: absolute;
  top: 0;
  left: 16px;
  right: 16px;
  height: 0;
  background: var(--primary-light);
  border-radius: var(--radius-md);
  opacity: 0;
  z-index: 1;
  pointer-events: none;
  will-change: transform, height, opacity;
}

.sidebar-nav {
  padding: 20px 16px;
  flex: 1;
  display: flex;
  flex-direction: column;
}

.nav-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 16px;
  color: var(--text-secondary);
  text-decoration: none;
  transition: color var(--transition-fast);
  cursor: pointer;
  font-weight: 500;
  position: relative;
  z-index: 2;
}

.nav-item:hover {
  color: var(--text-primary);
}

.nav-item.active {
  color: var(--primary-color);
  text-shadow: var(--primary-color)  0 0 3em;
  font-weight: 600;
}

.nav-icon {
  font-size: 18px;
  flex-shrink: 0;
}

.nav-text {
  font-size: 14px;
}

.sidebar-footer {
  margin-top: auto;
  padding-top: 16px;
  border-top: 1px solid var(--border-light);
}

@media (prefers-color-scheme: dark) {
  .nav-item:hover {
    background: var(--bg-hover);
  }

  .nav-item.active {
    color: var(--primary-color);
  }
}
</style>
