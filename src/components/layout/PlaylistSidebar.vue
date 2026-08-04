<script setup>
/**
 * PlaylistSidebar - 侧边栏歌单区（左栏）
 *
 * 仅保留框架与外层容器，逻辑统一收敛到 composable，三段区分别由
 * 子组件呈现：EndlessPlaySection / PlaylistSection / RecentTracksSection。
 */
import { useRoute } from 'vue-router';
import { computed } from 'vue';
import LayoutStore from '@/stores/layout.js';
import PlayerStore from '@/stores/player/index.js';
import { usePlaylistSidebar } from '@/composables/usePlaylistSidebar.js';
import EndlessPlaySection from './playlist-sidebar/EndlessPlaySection.vue';
import PlaylistSection from './playlist-sidebar/PlaylistSection.vue';
import RecentTracksSection from './playlist-sidebar/RecentTracksSection.vue';
import SidebarToast from './playlist-sidebar/SidebarToast.vue';

// 与 tokens.css 的 --side-nav-width / --playlist-sidebar-width 保持一致
const SIDE_NAV_WIDTH = 64;
const PLAYLIST_SIDEBAR_WIDTH = 264;

const route = useRoute();

// 折叠时用 transform 滑动（GPU 合成）。nav 折叠会让本栏左移 64px 填位，
// 本栏折叠再额外左移 100%（264px），两者叠加，避免改动 width 触发重排。
const sidebarTransform = computed(() => {
  const nav = LayoutStore.sideNavCollapsed.value ? -SIDE_NAV_WIDTH : 0;
  const playlist = LayoutStore.playlistSidebarCollapsed.value ? -PLAYLIST_SIDEBAR_WIDTH : 0;
  return `translateX(${nav + playlist}px)`;
});
const {
  sectionOpen,
  neteaseEnabled,
  playlists,
  recentTracks,
  busyItem,
  toast,
  sportBpmOpen,
  sportBpm,
  currentNeteaseSongId,
  endlessMode,
  startSmartMix,
  startShuffle,
  startPersonalFm,
  startPrivateDj,
  startSportRun,
  startSimilar,
  playRecent,
  handleClearRecent,
  handleCreatePlaylist,
} = usePlaylistSidebar();
</script>

<template>
  <aside
    class="playlist-sidebar"
    :class="{ collapsed: LayoutStore.playlistSidebarCollapsed.value }"
    :style="{ transform: sidebarTransform }"
  >
    <div class="sidebar-scroll">
      <!-- ═══ 无尽播放 ═══ -->
      <EndlessPlaySection
        :open="sectionOpen.endless"
        :busy-item="busyItem"
        :netease-enabled="neteaseEnabled"
        :endless-mode="endlessMode"
        :sport-bpm-open="sportBpmOpen"
        :sport-bpm="sportBpm"
        :current-netease-song-id="currentNeteaseSongId"
        @toggle="sectionOpen.endless = !sectionOpen.endless"
        @toggle-bpm="sportBpmOpen = !sportBpmOpen"
        @update:bpm="sportBpm = $event"
        @start-smart-mix="startSmartMix"
        @start-shuffle="startShuffle"
        @start-personal-fm="startPersonalFm"
        @start-private-dj="startPrivateDj"
        @start-sport-run="startSportRun"
        @start-similar="startSimilar"
      />

      <!-- ═══ 播放列表 ═══ -->
      <PlaylistSection
        :open="sectionOpen.playlists"
        :playlists="playlists"
        :active-path="route.path"
        @toggle="sectionOpen.playlists = !sectionOpen.playlists"
        @create="handleCreatePlaylist"
      />

      <!-- ═══ 最近播放 ═══ -->
      <RecentTracksSection
        :open="sectionOpen.recent"
        :recent-tracks="recentTracks"
        :current-track-id="PlayerStore.state.currentTrack?.id"
        @toggle="sectionOpen.recent = !sectionOpen.recent"
        @clear="handleClearRecent"
        @play="playRecent"
      />
    </div>

    <!-- 轻提示 -->
    <SidebarToast :toast="toast" />
  </aside>
</template>

<style scoped>
.playlist-sidebar {
  position: absolute;
  top: 0;
  bottom: 0;
  left: var(--side-nav-width);
  width: var(--playlist-sidebar-width);
  z-index: 20;
  background: var(--bg-secondary);
  border-right: 1px solid var(--border-light);
  display: flex;
  flex-direction: column;
  overflow: hidden;
  /* 折叠用 transform 滑动（GPU 合成），不再改 width，避免触发 flex/虚拟列表重排 */
  transition: transform var(--transition-normal);
  will-change: transform;
}

.playlist-sidebar.collapsed {
  border-right: none;
}

.sidebar-scroll {
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
  padding: 8px 8px 20px;
}

@media (max-width: 767px) {
  .playlist-sidebar {
    display: none;
  }
}
</style>