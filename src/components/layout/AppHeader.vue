<script setup>
import { ref, computed, onMounted, useTemplateRef } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import WindowControls from '@/components/WindowControls.vue';
import { usePlatform } from '@/composables/usePlatform.js';
import { useAnime } from '@/composables/useAnime.js';
import { useLiquidGlass, LIQUID_GLASS_PRESETS } from '@/composables/useLiquidGlass.js';
import LiquidGlassFilter from '@/components/ui/LiquidGlassFilter.vue';

const route = useRoute();
const router = useRouter();

const { isDesktop } = usePlatform();

const rootRef = useTemplateRef('root');
const { run } = useAnime(() => rootRef.value);

// header 三段区块错峰入场（fadeInDown + stagger）
onMounted(() => {
  run(({ animate, stagger, presets }) => {
    animate('.header-left, .header-center, .header-right', {
      ...presets.fadeInDown,
      delay: stagger(60),
      onComplete: () => {
        // 清除内联 transform，恢复 CSS 过渡
        rootRef.value?.querySelectorAll('.header-left, .header-center, .header-right').forEach((el) => {
          el.style.transform = '';
        });
      },
    });
  });
});

const pageTitle = computed(() => {
  return route.meta?.title || 'Chordial';
});

const searchQuery = ref('');

const emit = defineEmits(['search']);

const handleSearch = () => {
  const q = searchQuery.value.trim();
  if (!q) return;
  emit('search', q);
  router.push({ name: 'Search', query: { q } });
  searchQuery.value = '';
};

const goToSettings = () => {
  router.push('/settings');
};

// ── 液态玻璃效果（搜索框 + 图标按钮共用同一份 composable 实例） ──
// 三者使用同一 filterId 引用同一 SVG filter，避免重复定义；
// 视觉差异通过各自的 .search-box / .icon-btn CSS 覆盖 background/border 实现。
const { glassStyle: searchGlassStyle, filterProps: searchFilterProps } = useLiquidGlass({
  ...LIQUID_GLASS_PRESETS.user,
  elasticity: 0,           // 搜索框不做鼠标跟随
  interactive: false,
});

// 图标按钮：所有按钮共享同一 filter（同一 feDisplacementMap），
// 因此任一按钮 hover 都会触发共享滤镜的 scale 动画，所有按钮边缘折射同步增强。
const {
  glassStyle: iconBtnGlassStyle,
  onEnter: onIconBtnEnter,
  onLeave: onIconBtnLeave,
  onMouseMove: onIconBtnMouseMove,
  filterProps: iconBtnFilterProps,
} = useLiquidGlass({
  ...LIQUID_GLASS_PRESETS.user,
  borderRadius: '50%',
});
</script>

<template>
  <header ref="root" class="app-header">
    <!-- 窗口拖动区域 - 仅桌面端 -->
    <div v-if="isDesktop" class="drag-region"></div>

    <div class="header-left">
      <!-- <h1 class="app-title">Chordial</h1> -->
      <span class="page-title">{{ pageTitle }}</span>
    </div>

    <div class="header-center">
      <div
        class="search-box"
        :style="searchGlassStyle"
      >
        <i class="bi bi-search search-icon"></i>
        <input
          v-model="searchQuery"
          type="text"
          placeholder="搜索歌曲、歌手、专辑..."
          @keyup.enter="handleSearch"
        />
      </div>
    </div>

    <div class="header-right">
      <button
        class="icon-btn settings-btn"
        title="设置"
        :style="iconBtnGlassStyle"
        @click="goToSettings"
        @mouseenter="onIconBtnEnter"
        @mouseleave="onIconBtnLeave"
        @mousemove="onIconBtnMouseMove"
      >
        <i class="bi bi-gear"></i>
      </button>
      <button
        class="icon-btn notification-btn"
        title="通知"
        :style="iconBtnGlassStyle"
        @mouseenter="onIconBtnEnter"
        @mouseleave="onIconBtnLeave"
        @mousemove="onIconBtnMouseMove"
      >
        <i class="bi bi-bell"></i>
      </button>
      <!-- 窗口控制按钮 - 仅桌面端 -->
      <WindowControls v-if="isDesktop" />
    </div>

    <!-- 搜索框 + 图标按钮共用的液态玻璃 filter 定义 -->
    <LiquidGlassFilter v-bind="searchFilterProps" />
    <LiquidGlassFilter v-bind="iconBtnFilterProps" />
  </header>
</template>

<style scoped>
.app-header {
  height: var(--header-height);
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 20px 0 24px;
  padding-top: var(--safe-area-top);
  position: sticky;
  top: 0;
  z-index: 100;
  backdrop-filter: saturate(180%) blur(16px);
  -webkit-backdrop-filter: saturate(180%) blur(16px);
  background: var(--bg-glass);
  border-bottom: 1px solid var(--border-light);
  /* GPU: 限制重绘范围，减少合成器开销 */
  contain: layout paint;
  will-change: backdrop-filter;
}

/* 窗口拖动区域 */
.drag-region {
  position: absolute;
  top: 0;
  left: 0;
  right: 220px;
  height: var(--header-height);
  -webkit-app-region: drag;
  z-index: 0;
}

.header-left {
  display: flex;
  align-items: center;
  gap: 16px;
  position: relative;
  z-index: 1;
}

.app-title {
  font-size: 22px;
  font-weight: 700;
  background: var(--primary-gradient);
  -webkit-background-clip: text;
  -webkit-text-fill-color: transparent;
  background-clip: text;
  margin: 0;
  -webkit-app-region: no-drag;
  letter-spacing: -0.5px;
}

.page-title {
  font-size: 26px;
  font-weight: 900;
}

.header-center {
  flex: 1;
  max-width: 400px;
  margin: 0 24px;
  position: relative;
  z-index: 1;
}

.search-box {
  position: relative;
  display: flex;
  align-items: center;
  /* 液态玻璃底色：backdrop-filter 由 composable inline style 注入 */
  background: rgba(255, 255, 255, 0.22);
  border: 1px solid rgba(255, 255, 255, 0.35);
  border-radius: 19px;
  box-shadow:
    0 4px 12px rgba(0, 0, 0, 0.06),
    inset 0 1px 0 rgba(255, 255, 255, 0.45);
  /* GPU: 输入框 backdrop 独立合成层 */
  will-change: backdrop-filter;
  contain: paint;
}

.search-icon {
  position: absolute;
  left: 12px;
  font-size: 14px;
  color: var(--text-tertiary);
  pointer-events: none;
  z-index: 1;
}

.search-box input {
  width: 100%;
  height: 38px;
  padding: 0 16px 0 40px;
  border: none;
  background: transparent;
  color: var(--text-primary);
  font-size: 14px;
  font-weight: 400;
  transition: all var(--transition-normal);
  -webkit-app-region: no-drag;
  outline: none;
  font-family: inherit;
}

.search-box input:focus {
  /* 不再切换背景色：液态玻璃背景已统一，focus 仅通过 box-shadow 提示 */
  box-shadow: inset 0 0 0 2px var(--primary-color);
}

.search-box input::placeholder {
  color: var(--text-tertiary);
}

.header-right {
  display: flex;
  align-items: center;
  gap: 8px;
  position: relative;
  z-index: 1;
}

.icon-btn {
  width: 36px;
  height: 36px;
  border: 1px solid rgba(255, 255, 255, 0.35);
  border-radius: 50%;
  /* 液态玻璃底色：backdrop-filter 由 composable inline style 注入 */
  background: rgba(255, 255, 255, 0.22);
  color: var(--text-secondary);
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  transition: color var(--transition-fast), transform var(--transition-fast);
  -webkit-app-region: no-drag;
  /* GPU: 小图标 backdrop 最小化模糊半径 */
  will-change: backdrop-filter;
  contain: paint;
  padding: 0;
  font: inherit;
}

.icon-btn:hover {
  color: var(--text-primary);
}

.icon-btn:active {
  transform: scale(0.92);
}

.icon-btn i {
  font-size: 16px;
}

@media (max-width: 768px) {
  .app-header {
    padding: 0 0;
    padding-top: var(--safe-area-top);
  }

  .app-title {
    font-size: 18px;
  }

  .page-title {
    display: none;
  }

  .header-center {
    margin: 0 20px;
    max-width: none;
  }

  .search-box input {
    font-size: 13px;
    padding: 0 10px 0 32px;
  }

  .search-icon {
    left: 10px;
  }

  .icon-btn {
    width: 32px;
    height: 32px;
  }

  .icon-btn i {
    font-size: 14px;
  }

  /* 隐藏通知按钮，保留设置按钮 */
  .notification-btn {
    display: none;
  }
}

@media (max-width: 480px) {
  .app-header {
    padding: 0 0;
    padding-top: var(--safe-area-top);
  }

  .header-center {
    margin: 0 20px;
    max-width: none;
  }

  .search-box input::placeholder {
    font-size: 11px;
  }

  .search-box input {
    height: 32px;
    font-size: 12px;
  }

  .app-title {
    font-size: 16px;
  }
}

@media (prefers-color-scheme: dark) {
  .app-title {
    background: var(--primary-gradient);
    -webkit-background-clip: text;
    -webkit-text-fill-color: transparent;
    background-clip: text;
  }

  .search-box {
    background: rgba(40, 40, 45, 0.5);
    border-color: rgba(255, 255, 255, 0.15);
    box-shadow:
      0 4px 12px rgba(0, 0, 0, 0.3),
      inset 0 1px 0 rgba(255, 255, 255, 0.1);
  }

  .search-box input:focus {
    box-shadow: inset 0 0 0 2px var(--primary-color);
  }

  .icon-btn {
    background: rgba(40, 40, 45, 0.5);
    border-color: rgba(255, 255, 255, 0.15);
    color: var(--text-secondary);
  }

  .icon-btn:hover {
    color: var(--text-primary);
  }
}
</style>
