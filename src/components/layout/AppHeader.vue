<script setup>
import { ref, computed, onMounted, useTemplateRef } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import WindowControls from '@/components/WindowControls.vue';
import { usePlatform } from '@/composables/usePlatform.js';
import { useAnime } from '@/composables/useAnime.js';
import LiquidGlass from '@/components/ui/LiquidGlass.vue';

const route = useRoute();
const router = useRouter();

const { isDesktop } = usePlatform();

const rootRef = useTemplateRef('root');
const dragRegionRef = useTemplateRef('dragRegion');
const { run } = useAnime(() => rootRef.value);

// header 三段区块错峰入场（fadeInDown + stagger）
onMounted(() => {
  run(({ animate, stagger, presets }) => {
    animate('.header-left, .header-center, .header-right', {
      ...presets.fadeInDown,
      delay: stagger(60),
      onComplete: () => {
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
</script>

<template>
  <!--
    AppHeader - 浮动胶囊式顶栏
    - header 容器：sticky 全宽，仅负责布局占位、safe-area-top padding 和 drag region
    - 顶部 drag region：全宽覆盖，作为窗口拖动区 + LiquidGlass 的 mouseContainer
    - 浮动胶囊：LiquidGlass 包裹的 page-title + 搜索框 + 图标按钮 + WindowControls
      elasticity 鼠标跟随响应整个 drag region 范围
  -->
  <header ref="root" class="app-header">
    <!-- 窗口拖动区域 - 仅桌面端，全宽覆盖顶部 -->
    <div v-if="isDesktop" ref="dragRegion" class="drag-region"></div>

    <LiquidGlass
      :corner-radius="999"
      :elasticity="0.35"
      :mouse-container="dragRegionRef"
      padding="0 16px"
      class="header-pill"
    >
      <div class="header-content">
        <div class="header-left">
          <span class="page-title">{{ pageTitle }}</span>
        </div>

        <div class="header-center">
          <div class="search-box">
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
          <button class="icon-btn settings-btn" title="设置" @click="goToSettings">
            <i class="bi bi-gear"></i>
          </button>
          <button class="icon-btn notification-btn" title="通知">
            <i class="bi bi-bell"></i>
          </button>
          <WindowControls v-if="isDesktop" />
        </div>
      </div>
    </LiquidGlass>
  </header>
</template>

<style scoped>
.app-header {
  height: var(--header-height);
  padding: 0 16px;
  padding-top: var(--safe-area-top);
  position: sticky;
  top: 0;
  z-index: 100;
  /* header 容器不再提供 backdrop-filter，玻璃效果由浮动胶囊提供 */
  background: transparent;
  border-bottom: none;
  display: flex;
  align-items: center;
  justify-content: center;
}

/* 窗口拖动区域 - 全宽覆盖顶部 */
.drag-region {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  height: calc(var(--header-height) + var(--safe-area-top));
  -webkit-app-region: drag;
  z-index: 0;
  pointer-events: none;
}

/* 浮动胶囊 */
.header-pill {
  position: relative;
  z-index: 1;
  width: 100%;
  max-width: 1200px;
  height: var(--header-height);
  display: flex;
  align-items: center;
  /* 让胶囊内内容撑开高度 */
  -webkit-app-region: no-drag;
}

.header-content {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
  height: 100%;
  gap: 16px;
}

.header-left {
  display: flex;
  align-items: center;
  gap: 16px;
  flex-shrink: 0;
}

.page-title {
  font-size: 22px;
  font-weight: 800;
  white-space: nowrap;
}

.header-center {
  flex: 1;
  max-width: 400px;
  min-width: 0;
}

.search-box {
  position: relative;
  display: flex;
  align-items: center;
  width: 100%;
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
  height: 36px;
  padding: 0 16px 0 36px;
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
  /* focus 仅通过底部下划线提示 */
}

.search-box input::placeholder {
  color: var(--text-tertiary);
}

.header-right {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

.icon-btn {
  width: 34px;
  height: 34px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  transition: color var(--transition-fast), transform var(--transition-fast);
  -webkit-app-region: no-drag;
  padding: 0;
  font: inherit;
  border-radius: 50%;
}

.icon-btn:hover {
  color: var(--text-primary);
  background: rgba(127, 127, 127, 0.1);
}

.icon-btn:active {
  transform: scale(0.92);
}

.icon-btn i {
  font-size: 15px;
}

@media (max-width: 768px) {
  .app-header {
    padding: 0 12px;
    padding-top: var(--safe-area-top);
  }

  .page-title {
    display: none;
  }

  .header-center {
    margin: 0;
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
  .search-box input::placeholder {
    font-size: 11px;
  }

  .search-box input {
    height: 32px;
    font-size: 12px;
  }
}

@media (prefers-color-scheme: dark) {
  .icon-btn:hover {
    background: rgba(255, 255, 255, 0.08);
  }
}
</style>
