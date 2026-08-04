<script setup>
import { ref, shallowRef, computed, onMounted, watch } from 'vue';
import { useRouter, useRoute } from 'vue-router';
import { HugeiconsIcon } from '@hugeicons/vue';
import {
  RepeatIcon,
  ShuffleIcon,
  CrownIcon,
  RadioIcon,
  MicIcon,
  MapPinIcon,
  MusicNote01Icon,
  Playlist01Icon,
  Clock01Icon,
  AddCircleIcon,
  ChevronDownIcon,
  ChevronRightIcon,
  ListMusicIcon,
} from '@hugeicons/core-free-icons';
import LayoutStore from '@/stores/layout.js';
import PlayerStore from '@/stores/player/index.js';
import * as playlistApi from '@/api/musicSource/playlists.js';
import * as neteaseRadio from '@/api/musicSource/neteaseRadio.js';
import { useMusicSources } from '@/composables/useMusicSources.js';
import { useEndlessPlay } from '@/composables/useEndlessPlay.js';
import { useFavorites, getRecentTracks, clearRecentPlays, recordPlay } from '@/composables/useFavorites.js';

const router = useRouter();
const route = useRoute();

const { sources, refreshSources, loaded: sourcesLoaded } = useMusicSources();
const endless = useEndlessPlay();

// 网易云是否启用
const neteaseEnabled = computed(() => {
  const s = (sources.value || []).find((x) => x.id === 'netease');
  return !!s?.enabled;
});

// ── 折叠区段状态 ───────────────────────────────────────
const sectionOpen = ref({
  endless: true,
  playlists: true,
  recent: true,
});

// ── 播放列表 ───────────────────────────────────────────
const playlists = shallowRef([]);
async function loadPlaylists() {
  try {
    playlists.value = await playlistApi.list();
  } catch (e) {
    /* ignore */
  }
}

// ── 最近播放 ───────────────────────────────────────────
const recentTracks = shallowRef([]);
async function loadRecent() {
  recentTracks.value = await getRecentTracks(30);
}
const { load } = useFavorites();

// 监听路由与库变更刷新歌单
watch(() => route.path, () => {
  if (route.path === '/playlists' || route.path.startsWith('/playlists/')) {
    loadPlaylists();
  }
});

// 最近播放数据变化时刷新
watch(() => PlayerStore.state.currentTrack, (track) => {
  if (track?.id) {
    recordPlay(track);
    loadRecent();
  }
});

onMounted(async () => {
  refreshSources();
  loadPlaylists();
  await load();
  await loadRecent();
});

// ── 无尽播放 / 电台 交互状态 ───────────────────────────
const busyItem = ref('');       // 正在启动的项目 key
const toast = ref('');
let toastTimer = null;
function showToast(msg) {
  toast.value = msg;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { toast.value = ''; }, 2600);
}

async function runStart(key, fn) {
  busyItem.value = key;
  try {
    const n = await fn();
    showToast(`已开始播放（队列 ${n} 首）`);
  } catch (e) {
    showToast(e?.message || '启动失败');
  } finally {
    busyItem.value = '';
  }
}

const startSmartMix = () => runStart('smart-mix', () => endless.start('smart-mix'));
const startShuffle = () => runStart('shuffle', () => endless.start('shuffle'));

// 私人 FM
const startPersonalFm = () =>
  runStart('personal-fm', async () => {
    const songs = await neteaseRadio.personalFm(10);
    if (!songs.length) throw new Error('私人 FM 返回为空');
    PlayerStore.setPlaylist(songs);
    PlayerStore.play(songs[0], songs);
    return songs.length;
  });

// 私人 DJ
const startPrivateDj = () =>
  runStart('private-dj', async () => {
    const { songs } = await neteaseRadio.privateDj();
    if (!songs.length) throw new Error('私人 DJ 返回为空');
    PlayerStore.setPlaylist(songs);
    PlayerStore.play(songs[0], songs);
    return songs.length;
  });

// 跑步漫游（bpm 输入）
const sportBpmOpen = ref(false);
const sportBpm = ref(150);
const startSportRun = () =>
  runStart('sport-run', async () => {
    const songs = await neteaseRadio.sportRun(sportBpm.value);
    if (!songs.length) throw new Error('跑步漫游返回为空');
    PlayerStore.setPlaylist(songs);
    PlayerStore.play(songs[0], songs);
    sportBpmOpen.value = false;
    return songs.length;
  });

// 相似音乐（取当前曲目的网易云 id）
const currentNeteaseSongId = computed(() => {
  const t = PlayerStore.state.currentTrack;
  const sid = t?.sourceIds?.find((s) => s?.sourceName === 'netease' && s?.entityType === 'Song');
  return sid ? sid.entityId.replace(/^song:/, '') : null;
});
const startSimilar = () =>
  runStart('similar', async () => {
    const id = currentNeteaseSongId.value;
    if (!id) throw new Error('当前歌曲没有网易云来源');
    const songs = await neteaseRadio.similarSongs(id, 10);
    if (!songs.length) throw new Error('相似歌曲返回为空');
    PlayerStore.setPlaylist(songs);
    PlayerStore.play(songs[0], songs);
    return songs.length;
  });

// 最近播放条目播放
async function playRecent(track) {
  const tracks = recentTracks.value;
  PlayerStore.play(track, tracks);
}

// 清空最近播放
async function handleClearRecent() {
  await clearRecentPlays();
  await loadRecent();
}

// 创建歌单
async function handleCreatePlaylist() {
  const name = window.prompt('新歌单名称：', '我的歌单');
  if (!name || !name.trim()) return;
  try {
    await playlistApi.create(name.trim());
    await loadPlaylists();
    router.push('/playlists');
  } catch (e) {
    showToast(e?.message || '创建失败');
  }
}
</script>

<template>
  <aside
    class="playlist-sidebar"
    :class="{ collapsed: LayoutStore.playlistSidebarCollapsed.value }"
  >
    <div class="sidebar-scroll">
      <!-- ═══ 无尽播放 ═══ -->
      <section class="sidebar-section">
        <button class="section-title-btn" @click="sectionOpen.endless = !sectionOpen.endless">
          <HugeiconsIcon :icon="RepeatIcon" :size="15" class="title-icon" />
          <span>无尽播放</span>
          <HugeiconsIcon
            :icon="sectionOpen.endless ? ChevronDownIcon : ChevronRightIcon"
            :size="14"
            class="title-chevron"
          />
        </button>

        <div v-show="sectionOpen.endless" class="section-body">
          <!-- 智能混音 -->
          <button class="endless-item" :disabled="busyItem === 'smart-mix'" @click="startSmartMix">
            <span class="item-icon hot">
              <HugeiconsIcon :icon="CrownIcon" :size="16" />
            </span>
            <span class="item-text">
              <span class="item-name">智能混音</span>
              <span class="item-desc">常听高频 + 其余曲库交错循环</span>
            </span>
            <span v-if="endless.endlessMode.value === 'smart-mix'" class="live-dot">播放中</span>
          </button>

          <!-- 完全随机 -->
          <button class="endless-item" :disabled="busyItem === 'shuffle'" @click="startShuffle">
            <span class="item-icon">
              <HugeiconsIcon :icon="ShuffleIcon" :size="16" />
            </span>
            <span class="item-text">
              <span class="item-name">完全随机</span>
              <span class="item-desc">从音乐库全库随机播放</span>
            </span>
            <span v-if="endless.endlessMode.value === 'shuffle'" class="live-dot">播放中</span>
          </button>

          <!-- 网易云电台（启用网易云来源时显示） -->
          <template v-if="neteaseEnabled">
            <div class="sub-divider"></div>
            <div class="sub-caption">
              <HugeiconsIcon :icon="RadioIcon" :size="13" />
              <span>网易云电台</span>
            </div>

            <button class="endless-item" :disabled="busyItem === 'personal-fm'" @click="startPersonalFm">
              <span class="item-icon netease">
                <HugeiconsIcon :icon="MicIcon" :size="16" />
              </span>
              <span class="item-text">
                <span class="item-name">私人 FM</span>
                <span class="item-desc">智能推荐符合口味的歌曲</span>
              </span>
            </button>

            <button class="endless-item" :disabled="busyItem === 'private-dj'" @click="startPrivateDj">
              <span class="item-icon netease">
                <HugeiconsIcon :icon="MusicNote01Icon" :size="16" />
              </span>
              <span class="item-text">
                <span class="item-name">私人 DJ</span>
                <span class="item-desc">AI 声音推荐 + 歌曲</span>
              </span>
            </button>

            <div class="endless-item bpm-item">
              <button
                class="bpm-trigger"
                :disabled="busyItem === 'sport-run'"
                @click="sportBpmOpen = !sportBpmOpen"
              >
                <span class="item-icon netease">
                  <HugeiconsIcon :icon="MapPinIcon" :size="16" />
                </span>
                <span class="item-text">
                  <span class="item-name">跑步漫游</span>
                  <span class="item-desc">按步频匹配歌曲</span>
                </span>
              </button>
              <div v-if="sportBpmOpen" class="bpm-control" @click.stop>
                <input
                  v-model.number="sportBpm"
                  type="number"
                  min="60"
                  max="220"
                  step="5"
                  placeholder="步频 BPM"
                />
                <button class="bpm-go" @click="startSportRun">开始</button>
              </div>
            </div>

            <button
              class="endless-item"
              :disabled="busyItem === 'similar' || !currentNeteaseSongId"
              :title="currentNeteaseSongId ? '根据当前歌曲推荐相似歌曲' : '当前歌曲没有网易云来源'"
              @click="startSimilar"
            >
              <span class="item-icon netease">
                <HugeiconsIcon :icon="RadioIcon" :size="16" />
              </span>
              <span class="item-text">
                <span class="item-name">相似音乐</span>
                <span class="item-desc">基于当前歌曲推荐</span>
              </span>
            </button>
          </template>
        </div>
      </section>

      <!-- ═══ 播放列表 ═══ -->
      <section class="sidebar-section">
        <div class="section-title-row">
          <button class="section-title-btn grow" @click="sectionOpen.playlists = !sectionOpen.playlists">
            <HugeiconsIcon :icon="Playlist01Icon" :size="15" class="title-icon" />
            <span>播放列表</span>
            <HugeiconsIcon
              :icon="sectionOpen.playlists ? ChevronDownIcon : ChevronRightIcon"
              :size="14"
              class="title-chevron"
            />
          </button>
          <button class="section-add-btn" title="新建歌单" @click="handleCreatePlaylist">
            <HugeiconsIcon :icon="AddCircleIcon" :size="15" />
          </button>
        </div>

        <div v-show="sectionOpen.playlists" class="section-body">
          <div v-if="playlists.length === 0" class="section-empty">暂无歌单</div>
          <router-link
            v-for="pl in playlists"
            :key="pl.id"
            :to="`/playlists/${pl.id}`"
            class="playlist-item"
            :class="{ active: route.path === `/playlists/${pl.id}` }"
          >
            <span class="pl-cover">
              <img v-if="pl.cover_url" :src="pl.cover_url" :alt="pl.name" loading="lazy" />
              <HugeiconsIcon v-else :icon="ListMusicIcon" :size="15" class="pl-cover-ph" />
            </span>
            <span class="pl-name" :title="pl.name">{{ pl.name }}</span>
            <span class="pl-count">{{ pl.song_ids?.length ?? 0 }}</span>
          </router-link>
        </div>
      </section>

      <!-- ═══ 最近播放 ═══ -->
      <section class="sidebar-section">
        <div class="section-title-row">
          <button class="section-title-btn grow" @click="sectionOpen.recent = !sectionOpen.recent">
            <HugeiconsIcon :icon="Clock01Icon" :size="15" class="title-icon" />
            <span>最近播放</span>
            <HugeiconsIcon
              :icon="sectionOpen.recent ? ChevronDownIcon : ChevronRightIcon"
              :size="14"
              class="title-chevron"
            />
          </button>
          <button
            v-if="recentTracks.length"
            class="section-clear-btn"
            title="清空最近播放"
            @click="handleClearRecent"
          >
            清空
          </button>
        </div>

        <div v-show="sectionOpen.recent" class="section-body">
          <div v-if="recentTracks.length === 0" class="section-empty">暂无播放记录</div>
          <button
            v-for="t in recentTracks"
            :key="t.id"
            class="recent-item"
            :class="{ now: PlayerStore.state.currentTrack?.id === t.id }"
            @click="playRecent(t)"
          >
            <span class="recent-title" :title="t.title">{{ t.title }}</span>
            <span class="recent-artist">{{ t.getDisplayArtist ? t.getDisplayArtist() : t.artist }}</span>
          </button>
        </div>
      </section>
    </div>

    <!-- 轻提示 -->
    <transition name="toast-pop">
      <div v-if="toast" class="sidebar-toast">{{ toast }}</div>
    </transition>
  </aside>
</template>

<style scoped>
.playlist-sidebar {
  width: var(--playlist-sidebar-width);
  flex-shrink: 0;
  background: var(--bg-secondary);
  border-right: 1px solid var(--border-light);
  display: flex;
  flex-direction: column;
  position: relative;
  overflow: hidden;
  transition: width var(--transition-normal);
}

.playlist-sidebar.collapsed {
  width: 0;
  border-right: none;
}

.sidebar-scroll {
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
  padding: 8px 8px 20px;
}

/* ── 区段 ─────────────────────────────────────────── */
.sidebar-section {
  margin-bottom: 4px;
}

.section-title-row {
  display: flex;
  align-items: center;
  gap: 4px;
}

.section-title-btn {
  display: flex;
  align-items: center;
  gap: 7px;
  flex: 1;
  min-width: 0;
  padding: 6px 8px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--font-size-sm);
  font-weight: 600;
  letter-spacing: 0.03em;
  cursor: pointer;
  border-radius: var(--radius-sm);
  text-transform: uppercase;
  transition: color var(--transition-fast), background var(--transition-fast);
}

.section-title-btn.grow {
  flex: 1;
}

.section-title-btn:hover {
  color: var(--text-primary);
  background: var(--bg-hover);
}

.title-icon {
  color: var(--text-tertiary);
  flex-shrink: 0;
}

.title-chevron {
  color: var(--text-tertiary);
  margin-left: auto;
  flex-shrink: 0;
}

.section-add-btn {
  width: 26px;
  height: 26px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  border-radius: var(--radius-sm);
  transition: color var(--transition-fast), background var(--transition-fast);
}

.section-add-btn:hover {
  color: var(--text-primary);
  background: var(--bg-hover);
}

.section-clear-btn {
  border: none;
  background: transparent;
  color: var(--text-tertiary);
  font-size: var(--font-size-xs);
  cursor: pointer;
  padding: 4px 6px;
  border-radius: var(--radius-sm);
  transition: color var(--transition-fast);
}

.section-clear-btn:hover {
  color: var(--text-primary);
}

.section-body {
  padding: 2px 0 6px;
}

.section-empty {
  padding: 8px 10px;
  font-size: var(--font-size-sm);
  color: var(--text-tertiary);
}

/* ── 无尽播放条目 ─────────────────────────────────── */
.endless-item {
  display: flex;
  align-items: center;
  gap: 9px;
  width: 100%;
  padding: 7px 8px;
  border: none;
  border-radius: var(--radius-md);
  background: transparent;
  color: var(--text-primary);
  cursor: pointer;
  text-align: left;
  transition: background var(--transition-fast);
}

.endless-item:hover {
  background: var(--bg-hover);
}

.endless-item:disabled {
  opacity: 0.55;
  cursor: default;
}

.item-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border-radius: var(--radius-sm);
  background: var(--bg-tertiary);
  color: var(--text-secondary);
  flex-shrink: 0;
}

.item-icon.hot {
  background: var(--primary-light);
  color: var(--primary-color);
}

.item-icon.netease {
  background: rgba(197, 17, 17, 0.10);
  color: #c51111;
}

.item-text {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}

.item-name {
  font-size: var(--font-size-base);
  font-weight: 500;
  line-height: 1.3;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.item-desc {
  font-size: var(--font-size-xs);
  color: var(--text-tertiary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.live-dot {
  flex-shrink: 0;
  font-size: var(--font-size-xs);
  font-weight: 600;
  color: var(--primary-color);
  padding: 2px 6px;
  border-radius: 10px;
  background: var(--primary-light);
}

.sub-divider {
  height: 1px;
  background: var(--border-light);
  margin: 8px 8px 6px;
}

.sub-caption {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 8px 6px;
  font-size: var(--font-size-xs);
  font-weight: 600;
  color: var(--text-tertiary);
  letter-spacing: 0.04em;
  text-transform: uppercase;
}

/* 跑步漫游 bpm */
.bpm-item {
  display: block;
  padding: 0;
}

.bpm-trigger {
  display: flex;
  align-items: center;
  gap: 9px;
  width: 100%;
  padding: 7px 8px;
  border: none;
  background: transparent;
  color: var(--text-primary);
  cursor: pointer;
  text-align: left;
  border-radius: var(--radius-md);
}

.bpm-trigger:hover {
  background: var(--bg-hover);
}

.bpm-control {
  display: flex;
  gap: 6px;
  padding: 4px 8px 8px 43px;
}

.bpm-control input {
  width: 72px;
  padding: 5px 8px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--bg-primary);
  color: var(--text-primary);
  font-size: var(--font-size-sm);
  outline: none;
}

.bpm-control input:focus {
  border-color: var(--primary-color);
}

.bpm-go {
  padding: 5px 12px;
  border: none;
  border-radius: var(--radius-sm);
  background: var(--primary-color);
  color: var(--primary-contrast);
  font-size: var(--font-size-sm);
  font-weight: 500;
  cursor: pointer;
}

/* ── 播放列表条目 ─────────────────────────────────── */
.playlist-item {
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 6px 8px;
  border-radius: var(--radius-md);
  color: var(--text-primary);
  text-decoration: none;
  transition: background var(--transition-fast);
}

.playlist-item:hover {
  background: var(--bg-hover);
  text-decoration: none;
}

.playlist-item.active {
  background: var(--primary-light);
  color: var(--primary-color);
}

.pl-cover {
  width: 26px;
  height: 26px;
  border-radius: var(--radius-sm);
  background: var(--bg-tertiary);
  overflow: hidden;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
}

.pl-cover img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.pl-cover-ph {
  color: var(--text-tertiary);
}

.pl-name {
  flex: 1;
  min-width: 0;
  font-size: var(--font-size-base);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.pl-count {
  font-size: var(--font-size-xs);
  color: var(--text-tertiary);
  flex-shrink: 0;
}

/* ── 最近播放条目 ─────────────────────────────────── */
.recent-item {
  display: flex;
  flex-direction: column;
  width: 100%;
  padding: 6px 8px;
  border: none;
  border-radius: var(--radius-md);
  background: transparent;
  color: var(--text-primary);
  cursor: pointer;
  text-align: left;
  transition: background var(--transition-fast);
}

.recent-item:hover {
  background: var(--bg-hover);
}

.recent-item.now {
  background: var(--primary-light);
}

.recent-item.now .recent-title {
  color: var(--primary-color);
}

.recent-title {
  font-size: var(--font-size-base);
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.recent-artist {
  font-size: var(--font-size-xs);
  color: var(--text-tertiary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* ── 轻提示 ───────────────────────────────────────── */
.sidebar-toast {
  position: absolute;
  left: 12px;
  right: 12px;
  bottom: 12px;
  padding: 8px 12px;
  background: var(--text-primary);
  color: var(--primary-contrast);
  font-size: var(--font-size-sm);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-md);
  z-index: 20;
}

.toast-pop-enter-active,
.toast-pop-leave-active {
  transition: opacity 0.2s, transform 0.2s;
}
.toast-pop-enter-from,
.toast-pop-leave-to {
  opacity: 0;
  transform: translateY(6px);
}

@media (max-width: 767px) {
  .playlist-sidebar {
    display: none;
  }
}
</style>
