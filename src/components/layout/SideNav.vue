<script setup>
import { computed } from 'vue';
import { useRoute } from 'vue-router';
import { HugeiconsIcon } from '@hugeicons/vue';
import {
  HeartIcon,
  UserGroupIcon,
  DiscIcon,
  Playlist01Icon,
  Settings01Icon,
  MoreHorizontalIcon,
} from '@hugeicons/core-free-icons';
import LayoutStore from '@/stores/layout.js';

const route = useRoute();

// 顶部主导航
const primaryItems = [
  { path: '/favorites', label: '偏好库', icon: HeartIcon },
  { path: '/artists', label: '全部歌手', icon: UserGroupIcon },
  { path: '/albums', label: '全部专辑', icon: DiscIcon },
  { path: '/playlists', label: '全部歌单', icon: Playlist01Icon },
];

// 底部辅助导航
const secondaryItems = [
  { path: '/settings', label: '设置', icon: Settings01Icon },
  { path: '/more', label: '更多', icon: MoreHorizontalIcon },
];

const isActive = (path) => {
  if (path === '/favorites') return route.path === '/favorites';
  return route.path.startsWith(path);
};
</script>

<template>
  <nav
    class="side-nav"
    :class="{ collapsed: LayoutStore.sideNavCollapsed.value }"
    aria-label="主导航"
  >
    <div class="nav-stack nav-primary">
      <router-link
        v-for="item in primaryItems"
        :key="item.path"
        :to="item.path"
        class="nav-icon-btn"
        :class="{ active: isActive(item.path) }"
      >
        <HugeiconsIcon :icon="item.icon" :size="20" />
        <span class="nav-tooltip">{{ item.label }}</span>
      </router-link>
    </div>

    <div class="nav-stack nav-secondary">
      <router-link
        v-for="item in secondaryItems"
        :key="item.path"
        :to="item.path"
        class="nav-icon-btn"
        :class="{ active: isActive(item.path) }"
      >
        <HugeiconsIcon :icon="item.icon" :size="20" />
        <span class="nav-tooltip">{{ item.label }}</span>
      </router-link>
    </div>
  </nav>
</template>

<style scoped>
.side-nav {
  position: absolute;
  top: 0;
  bottom: 0;
  left: 0;
  width: var(--side-nav-width);
  z-index: 30;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: space-between;
  padding: 10px 0;
  background: var(--bg-secondary);
  border-right: 1px solid var(--border-light);
  overflow: hidden;
  /* 折叠用 transform 滑动（GPU 合成），不再改 width，避免触发 flex 重排 */
  transition: transform var(--transition-normal);
  will-change: transform;
}

/* 悬停图标时放开裁剪，让 tooltip 能溢出显示（折叠动画仍保持 hidden，不受影响） */
.side-nav:has(.nav-icon-btn:hover) {
  overflow: visible;
}

.side-nav.collapsed {
  transform: translateX(-100%);
}

.nav-stack {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  width: 100%;
}

.nav-icon-btn {
  position: relative;
  width: 40px;
  height: 40px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-md);
  color: var(--text-secondary);
  text-decoration: none;
  transition: color var(--transition-fast), background var(--transition-fast);
}

.nav-icon-btn:hover {
  color: var(--text-primary);
  background: var(--bg-hover);
}

.nav-icon-btn.active {
  color: var(--primary-color);
  background: var(--primary-light);
}

/* ── hover tooltip ────────────────────────────────── */
.nav-tooltip {
  position: absolute;
  left: calc(100% + 10px);
  top: 50%;
  transform: translateY(-50%) translateX(-4px);
  padding: 5px 10px;
  background: var(--text-primary);
  color: var(--primary-contrast);
  font-size: var(--font-size-sm);
  font-weight: 500;
  line-height: 1.4;
  white-space: nowrap;
  border-radius: var(--radius-sm);
  box-shadow: var(--shadow-md);
  opacity: 0;
  pointer-events: none;
  transition: opacity var(--transition-fast), transform var(--transition-fast);
  z-index: 500;
}

.nav-icon-btn:hover .nav-tooltip {
  opacity: 1;
  transform: translateY(-50%) translateX(0);
}

@media (max-width: 767px) {
  .side-nav {
    display: none;
  }
}
</style>
