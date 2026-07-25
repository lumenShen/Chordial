<script setup>
import { useRoute } from 'vue-router';
import { computed, onMounted, useTemplateRef, watch, nextTick } from 'vue';
import PlayerStore from '@/stores/player.js';
import { useAnime } from '@/composables/useAnime.js';
import { ANIME_SPRINGS } from '@/utils/animePresets.js';
import LiquidGlass from '@/components/ui/LiquidGlass.vue';

const route = useRoute();

const hasCurrentTrack = computed(() => PlayerStore.hasCurrentTrack.value);

const rootRef = useTemplateRef('root');
const indicatorRef = useTemplateRef('indicator');
const { run, animate } = useAnime(() => rootRef.value);

// 使用 LiquidGlass 库组件提供液态玻璃效果（折射 + 像差 + 弹性 + 鼠标跟随）
// 圆角 = 999 形成完整胶囊形态，由 LiquidGlass 内部管理 SVG filter 与 hover 交互

// 导航项错峰入场（fadeInUp + stagger）
onMounted(() => {
  run(({ animate, stagger, presets }) => {
    animate('.bottom-nav-item', {
      ...presets.fadeInUp,
      delay: stagger(50),
      onComplete: () => {
        // 清除内联 transform，恢复 CSS :hover / .active 过渡
        rootRef.value?.querySelectorAll('.bottom-nav-item').forEach((el) => {
          el.style.transform = '';
        });
      },
    });
  });
  // 初始化 active 指示器
  nextTick(updateIndicator);
});

const navItems = [
  {
    path: '/',
    name: '首页',
    icon: 'bi-house'
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

// active 指示器滑动动画（anime.js spring 物理，替代 CSS transition 的生硬切换）
function updateIndicator() {
  const root = rootRef.value;
  const indicator = indicatorRef.value;
  if (!root || !indicator) return;
  const activeEl = root.querySelector('.bottom-nav-item.active');
  if (!activeEl) {
    // 无 active 项：隐藏指示器
    animate(indicator, { opacity: 0, duration: 200, ease: ANIME_SPRINGS.sensitive });
    return;
  }
  const navRect = root.getBoundingClientRect();
  const itemRect = activeEl.getBoundingClientRect();
  const x = itemRect.left - navRect.left;
  const w = itemRect.width;
  animate(indicator, {
    translateX: x,
    width: w,
    opacity: 1,
    duration: 420,
    ease: ANIME_SPRINGS.bouncy,
  });
}

// 路由切换 → FLIP 指示器滑动
watch(() => route.path, () => {
  nextTick(updateIndicator);
});
</script>

<template>
  <div ref="root" class="app-bottom-nav" :class="{ 'with-player': hasCurrentTrack }">
    <LiquidGlass
      :corner-radius="999"
      :elasticity="0.35"
      padding="0 16px"
      class="nav-glass"
    >
      <div ref="indicator" class="nav-indicator"></div>
      <router-link v-for="item in navItems" :key="item.path" :to="item.path" class="bottom-nav-item"
        :class="{ active: isActive(item.path) }">
        <i :class="item.icon" class="bottom-nav-icon"></i>
        <span class="bottom-nav-text">{{ item.name }}</span>
      </router-link>
    </LiquidGlass>
  </div>
</template>

<style scoped>
.app-bottom-nav {
  position: fixed;
  bottom: 1.5rem;
  /* 桌面端：居左浮动胶囊 */
  left: 1.5rem;
  z-index: 100;
  transition: bottom var(--transition-slow);
}

/* 玻璃容器：由 LiquidGlass 提供背景、边框、阴影、圆角、模糊与折射 */
.nav-glass {
  height: var(--bottom-nav-height);
  display: flex;
  align-items: center;
  gap: 4px;
  width: fit-content;
  max-width: calc(100vw - 3rem);
  padding-bottom: var(--safe-area-bottom);
  overflow: hidden;
  isolation: isolate;
}

/* active 指示器（绝对定位背景胶囊，由 anime.js animate translateX/width 滑动） */
.nav-indicator {
  position: absolute;
  left: 0;
  height: calc(var(--bottom-nav-height) * 0.72);
  width: 0;
  background: var(--primary-light);
  border-radius: var(--radius-md);
  opacity: 0;
  z-index: 0;
  pointer-events: none;
  will-change: transform, width;
}

.bottom-nav-item {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 0 18px;
  color: var(--text-secondary);
  text-decoration: none;
  transition: color var(--transition-fast);
  border-radius: var(--radius-md);
  flex: 0 0 auto;
  font-weight: 500;
  min-width: 0;
  position: relative;
  z-index: 1;
}

.bottom-nav-item:hover {
  color: var(--text-primary);
}

.bottom-nav-item.active {
  color: var(--primary-color);
  text-shadow: var(--primary-color) 0 0 2em;
}

.bottom-nav-icon {
  font-size: 22px;
}

.bottom-nav-text {
  font-size: 11px;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

@media (max-width: 767px) {
  .app-bottom-nav {
    /* 移动端：占满宽度减去边距，向上避让播放器控制栏 */
    right: 1rem;
    left: 1rem;
    transform: none;
    bottom: var(--player-bar-height, 100px);
  }

  .nav-glass {
    width: auto;
  }
}

@media (prefers-color-scheme: dark) {
  .bottom-nav-item.active {
    color: var(--primary-color);
  }
}
</style>
