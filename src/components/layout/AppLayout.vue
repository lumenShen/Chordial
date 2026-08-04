<script setup>
import { computed, watch, useTemplateRef, provide } from 'vue'
import { animate } from 'animejs';
import { ANIME_PRESETS } from '@/utils/animePresets.js';
import AppHeader from './AppHeader.vue';
import SideNav from './SideNav.vue';
import PlaylistSidebar from './PlaylistSidebar.vue';
import AppBottomBar from './AppBottomBar.vue';
import AppBottomNav from './AppBottomNav.vue';
import P2pMatchDialog from '@/components/p2p/P2pMatchDialog.vue';
import BatchDownloadManager from '@/components/common/BatchDownloadManager.vue';
import { PlayerControlBar } from '@/components/player';
import PlayerView from '@/views/PlayerView.vue';
import { PlayerStore } from '@/stores/player/index.js'
import { useCoverImage } from '@/composables/useCoverImage';
import { useBreakpoint } from '@/composables/useBreakpoint';

const currentTrack = computed(() => PlayerStore.state.currentTrack)
const { coverUrl: albumCoverUrl } = useCoverImage(currentTrack, 'small')
const isPlaying = computed(() => PlayerStore.state.isPlaying)
const isPlayerViewOpen = computed(() => PlayerStore.state.ui.isPlayerViewOpen)

// 主滚动容器：.content-wrapper（page 模式下虚拟列表会监听它的 scroll 事件）
// 提供给后代组件（如 TrackList）使用，避免双层滚动嵌套
const contentWrapperRef = useTemplateRef('contentWrapper');
provide('appScrollContainer', contentWrapperRef);

// 设备检测：useBreakpoint 统一监听视口，同步到 PlayerStore.ui.currentDevice
const { width: viewportWidth, isDesktop, isMobile } = useBreakpoint();
watch(viewportWidth, (w) => PlayerStore.detectDevice(w), { immediate: true });

// 路由切换动画：前页缩小淡出 + 后页放大淡入，同步进行，总时长 300ms
function onRouteEnter(el, done) {
  el.style.transformOrigin = '50% 100%';
  el.style.willChange = 'transform, opacity';
  animate(el, {
    ...ANIME_PRESETS.routeEnter,
    onComplete: () => {
      el.style.willChange = '';
      done();
    },
  });
}

function onRouteLeave(el, done) {
  const rect = el.getBoundingClientRect();
  el.style.position = 'fixed';
  el.style.top = `${rect.top}px`;
  el.style.left = `${rect.left}px`;
  el.style.width = `${rect.width}px`;
  el.style.height = `${rect.height}px`;
  el.style.margin = '0';
  el.style.boxSizing = 'border-box';
  el.style.pointerEvents = 'none';
  el.style.transformOrigin = '50% 100%';
  el.style.willChange = 'transform, opacity';
  animate(el, {
    ...ANIME_PRESETS.routeLeave,
    onComplete: () => {
      el.style.willChange = '';
      done();
    },
  });
}
</script>

<template>
  <div class="app-layout">
    <!-- 顶栏（全平台） -->
    <AppHeader />

    <!-- 主体：桌面端 左栏一 | 左栏二 | main；移动端仅 main -->
    <div class="layout-body">
      <SideNav v-if="!isMobile" />
      <PlaylistSidebar v-if="!isMobile" />
      <main class="main-content">
        <div class="content-wrapper" ref="contentWrapper">
          <router-view v-slot="{ Component }">
            <transition :css="false" @enter="onRouteEnter" @leave="onRouteLeave">
              <component :is="Component" />
            </transition>
          </router-view>
        </div>
      </main>
    </div>

    <!-- 底栏：桌面端全宽播放控制条；移动端悬浮播放条 -->
    <AppBottomBar v-if="!isMobile" />
    <PlayerControlBar
      v-if="isMobile"
      :album-cover-url="albumCoverUrl"
      :is-playing="isPlaying"
      @play="PlayerStore.resume"
      @pause="PlayerStore.pause"
      @next="PlayerStore.playNext"
    />

    <!-- 移动端底部导航 -->
    <AppBottomNav v-if="isMobile" />

    <!-- PlayerView 模态层（不再通过 router） -->
    <PlayerView v-if="isPlayerViewOpen" />

    <!-- P2P 匹配对话框 + 事件桥接 -->
    <P2pMatchDialog />

    <!-- 批量下载浮窗（监听 netease-download-progress 事件） -->
    <BatchDownloadManager />
  </div>
</template>

<style scoped>
.app-layout {
  height: 100vh;
  display: flex;
  flex-direction: column;
  background: var(--bg-primary);
  overflow: hidden;
  position: relative;
}

.layout-body {
  flex: 1;
  display: flex;
  overflow: hidden;
  min-height: 0;
}

.main-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  min-height: 0;
}

.content-wrapper {
  padding: 24px 28px calc(var(--bottom-bar-height) + 28px);
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
  min-height: 0;
  position: relative;
}

/* 移动端：无左栏，底部由 播放条 + 底部导航 让位 */
@media (max-width: 767px) {
  .content-wrapper {
    padding: 20px 16px calc(var(--bottom-nav-height) + var(--player-bar-height) + var(--safe-area-bottom) + 28px);
  }
}
</style>
