<template>
  <div class="settings-section">
    <h3 class="block-title">
      <HIcon name="cloud-arrow-down" />
      我的资源
      <span v-if="state.loading" class="loading-text">
        <HIcon name="arrow-clockwise" class="spinning" /> 加载中…
      </span>
    </h3>

    <div v-if="state.error" class="test-result fail">
      <HIcon name="x-circle-fill" />
      <span>{{ state.error }}</span>
      <button class="action-btn ghost" @click="load" style="margin-left: auto;">
        <HIcon name="arrow-clockwise" /> 重试
      </button>
    </div>

    <!-- 我创建的歌单 -->
    <div v-if="state.createdPlaylists.length" class="my-resource-group">
      <h4 class="group-title">
        <HIcon name="music-note-list" />
        我创建的歌单
        <span class="group-count">{{ state.createdPlaylists.length }}</span>
      </h4>
      <div class="resource-grid">
        <div v-for="item in state.createdPlaylists" :key="item.id" class="resource-card">
          <img v-if="item.coverImgUrl" :src="item.coverImgUrl" :alt="item.name" class="resource-cover" loading="lazy" />
          <div v-else class="resource-cover placeholder"><HIcon name="music-note-beamed" /></div>
          <div class="resource-info">
            <div class="resource-name" :title="item.name">{{ item.name }}</div>
            <div class="resource-meta">
              <span>{{ item.trackCount }} 首</span>
              <span v-if="item.playCount">· {{ formatCount(item.playCount) }} 次播放</span>
            </div>
          </div>
          <button
            class="action-btn primary small"
            :class="{ done: state.imported.has(item.id) }"
            :disabled="state.importing[item.id] === 'loading' || state.imported.has(item.id)"
            @click="importPlaylist(item)"
          >
            <i class="bi" :class="
              state.importing[item.id] === 'loading' ? 'bi-arrow-clockwise spinning' :
              state.imported.has(item.id) ? 'bi-check-lg' :
              state.importing[item.id] === 'error' ? 'bi-arrow-clockwise' :
              'bi-download'
            "></i>
            {{ state.imported.has(item.id) ? '已导入' : '导入' }}
          </button>
        </div>
      </div>
    </div>

    <!-- 收藏的歌单 -->
    <div v-if="state.subscribedPlaylists.length" class="my-resource-group">
      <h4 class="group-title">
        <HIcon name="heart" />
        收藏的歌单
        <span class="group-count">{{ state.subscribedPlaylists.length }}</span>
      </h4>
      <div class="resource-grid">
        <div v-for="item in state.subscribedPlaylists" :key="item.id" class="resource-card">
          <img v-if="item.coverImgUrl" :src="item.coverImgUrl" :alt="item.name" class="resource-cover" loading="lazy" />
          <div v-else class="resource-cover placeholder"><HIcon name="music-note-beamed" /></div>
          <div class="resource-info">
            <div class="resource-name" :title="item.name">{{ item.name }}</div>
            <div class="resource-meta">
              <span v-if="item.creator">by {{ item.creator.nickname }}</span>
              <span>· {{ item.trackCount }} 首</span>
            </div>
          </div>
          <button
            class="action-btn primary small"
            :class="{ done: state.imported.has(item.id) }"
            :disabled="state.importing[item.id] === 'loading' || state.imported.has(item.id)"
            @click="importPlaylist(item)"
          >
            <i class="bi" :class="
              state.importing[item.id] === 'loading' ? 'bi-arrow-clockwise spinning' :
              state.imported.has(item.id) ? 'bi-check-lg' :
              'bi-download'
            "></i>
            {{ state.imported.has(item.id) ? '已导入' : '导入' }}
          </button>
        </div>
      </div>
    </div>

    <!-- 收藏的专辑 -->
    <div v-if="state.albums.length" class="my-resource-group">
      <h4 class="group-title">
        <HIcon name="disc" />
        收藏的专辑
        <span class="group-count">{{ state.albums.length }} / {{ state.albumsTotal }}</span>
      </h4>
      <div class="resource-grid">
        <div v-for="item in state.albums" :key="item.id" class="resource-card">
          <img v-if="item.picUrl" :src="item.picUrl" :alt="item.name" class="resource-cover" loading="lazy" />
          <div v-else class="resource-cover placeholder"><HIcon name="disc" /></div>
          <div class="resource-info">
            <div class="resource-name" :title="item.name">{{ item.name }}</div>
            <div class="resource-meta">
              <span v-if="item.artist">{{ item.artist.name }}</span>
              <span>· {{ item.trackCount }} 首</span>
            </div>
          </div>
          <button
            class="action-btn primary small"
            :class="{ done: state.imported.has(item.id) }"
            :disabled="state.importing[item.id] === 'loading' || state.imported.has(item.id)"
            @click="importAlbum(item)"
          >
            <i class="bi" :class="
              state.importing[item.id] === 'loading' ? 'bi-arrow-clockwise spinning' :
              state.imported.has(item.id) ? 'bi-check-lg' :
              'bi-download'
            "></i>
            {{ state.imported.has(item.id) ? '已导入' : '导入' }}
          </button>
        </div>
      </div>
    </div>

    <!-- 三类都为空的提示 -->
    <div
      v-if="!state.loading && !state.error
        && !state.createdPlaylists.length
        && !state.subscribedPlaylists.length
        && !state.albums.length"
      class="empty-tip"
    >
      <HIcon name="inbox" />
      <p>未找到任何歌单或收藏的专辑</p>
    </div>
  </div>
</template>

<script setup>
import { ref, watch } from 'vue';
import * as neteaseApi from '@/api/musicSource/netease.js';

const props = defineProps({
  config: { type: Object, required: true },
});

const emit = defineEmits(['notify', 'nickname']);

// "我的资源"状态
const state = ref({
  loading: false,
  error: '',
  uid: 0,
  // 用户创建的歌单
  createdPlaylists: [],
  // 用户收藏的歌单
  subscribedPlaylists: [],
  // 用户收藏的专辑
  albums: [],
  albumsTotal: 0,
  // 各项导入中状态（key 为资源 id，value 为 'loading' | 'done' | 'error'）
  importing: {},
  // 已导入完成的资源 id 集合
  imported: new Set(),
});

function hasLoggedInCookie(cookie) {
  // MUSIC_U 是网易云登录态的核心 cookie
  return /(^|;\s*)MUSIC_U\s*=/.test((cookie || '').trim());
}

// 拉取"我的资源"：用户信息 → 并行拉取歌单 + 专辑
async function load() {
  state.value.loading = true;
  state.value.error = '';
  try {
    const accountResp = await neteaseApi.getUserAccount();
    const profile = accountResp.profile;
    if (!profile || !profile.userId) {
      state.value.error = '未获取到用户信息（cookie 可能已过期）';
      state.value.loading = false;
      return;
    }
    state.value.uid = profile.userId;
    emit('nickname', profile.nickname || '');

    // 并行拉取歌单 + 专辑
    const [playlists, albumsResp] = await Promise.all([
      neteaseApi.getUserPlaylists(profile.userId),
      neteaseApi.getUserAlbums(50, 0),
    ]);

    // 按 subscribed 字段区分创建/收藏
    state.value.createdPlaylists = (playlists || []).filter(
      (p) => p.subscribed === false || p.subscribed === null || p.subscribed === undefined
    );
    state.value.subscribedPlaylists = (playlists || []).filter(
      (p) => p.subscribed === true
    );
    state.value.albums = albumsResp.albums || [];
    state.value.albumsTotal = albumsResp.total || 0;
  } catch (e) {
    state.value.error = e.message || String(e);
  } finally {
    state.value.loading = false;
  }
}

function reset() {
  state.value = {
    loading: false,
    error: '',
    uid: 0,
    createdPlaylists: [],
    subscribedPlaylists: [],
    albums: [],
    albumsTotal: 0,
    importing: {},
    imported: new Set(),
  };
}

// 导入歌单到本地库（复用 savePlaylistToLibrary）
async function importPlaylist(item) {
  if (state.value.imported.has(item.id)) return;
  if (state.value.importing[item.id] === 'loading') return;
  state.value.importing[item.id] = 'loading';
  try {
    await neteaseApi.savePlaylistToLibrary(item.id);
    state.value.imported.add(item.id);
    state.value.importing[item.id] = 'done';
    emit('notify', `歌单「${item.name}」已导入`, 'success');
  } catch (e) {
    state.value.importing[item.id] = 'error';
    emit('notify', `导入失败：${e.message || e}`, 'error');
  }
}

// 导入专辑到本地库
async function importAlbum(item) {
  if (state.value.imported.has(item.id)) return;
  if (state.value.importing[item.id] === 'loading') return;
  state.value.importing[item.id] = 'loading';
  try {
    const r = await neteaseApi.saveAlbumToLibrary(item.id);
    state.value.imported.add(item.id);
    state.value.importing[item.id] = 'done';
    emit('notify', `专辑「${item.name}」已导入 ${r.imported_count}/${r.total_count} 首`, 'success');
  } catch (e) {
    state.value.importing[item.id] = 'error';
    emit('notify', `导入失败：${e.message || e}`, 'error');
  }
}

// 播放数格式化（万/亿）
function formatCount(n) {
  if (!n || n <= 0) return '0';
  if (n >= 100000000) return (n / 100000000).toFixed(1) + '亿';
  if (n >= 10000) return (n / 10000).toFixed(1) + '万';
  return String(n);
}

// 登录态变化：登录后自动拉取"我的资源"，退出登录时清空
watch(
  () => props.config.cookie,
  (cookie) => {
    if (hasLoggedInCookie(cookie) && state.value.uid === 0 && !state.value.loading) {
      load();
    } else if (!hasLoggedInCookie(cookie)) {
      reset();
    }
  },
  { immediate: true }
);
</script>

<style scoped>
/* ── 我的资源面板 ───────────────────────────────────────────────────── */
.my-resource-group {
  margin-top: 18px;
}

.group-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-secondary, #666);
  margin: 0 0 10px;
  display: flex;
  align-items: center;
  gap: 6px;
}

.group-count {
  background: var(--bg-tertiary, rgba(15, 15, 15, 0.05));
  color: var(--text-tertiary, #999);
  padding: 1px 8px;
  border-radius: 10px;
  font-size: 11px;
  font-weight: 600;
}

.resource-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
  gap: 10px;
}

.resource-card {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px;
  background: var(--bg-secondary, #fafafa);
  border: 1px solid var(--border-light, #e8e8e8);
  border-radius: 10px;
  transition: all 0.2s;
}

.resource-card:hover {
  border-color: var(--primary-color);
  background: var(--bg-hover);
}

.resource-cover {
  width: 56px;
  height: 56px;
  border-radius: 8px;
  object-fit: cover;
  flex-shrink: 0;
  background: var(--bg-tertiary, #f0f0f0);
}

.resource-cover.placeholder {
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-tertiary, #999);
  font-size: 22px;
}

.resource-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.resource-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary, #333);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.resource-meta {
  font-size: 11px;
  color: var(--text-tertiary, #999);
  display: flex;
  gap: 4px;
  flex-wrap: wrap;
}

.empty-tip {
  padding: 32px 16px;
  text-align: center;
  color: var(--text-tertiary, #999);
  font-size: 13px;
}

.empty-tip i {
  font-size: 32px;
  display: block;
  margin-bottom: 8px;
}

.empty-tip p {
  margin: 0;
}

.loading-text {
  font-size: 12px;
  color: var(--text-secondary, #666);
  font-weight: normal;
  margin-left: 8px;
}

.spinning {
  animation: spin 1s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

@media (max-width: 480px) {
  .resource-grid {
    grid-template-columns: 1fr;
  }
}
</style>