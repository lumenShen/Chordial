<script setup>
import { ref, shallowRef, computed, onMounted, useTemplateRef } from 'vue';
import { useRouter } from 'vue-router';
import { useAnime } from '@/composables/useAnime.js';
import * as playlistApi from '@/api/musicSource/playlists.js';

const router = useRouter();

const playlists = shallowRef([]);
const isLoading = ref(false);
const errorMsg = ref('');

const showCreateDialog = ref(false);
const newPlaylistName = ref('');
const newPlaylistDesc = ref('');
const creating = ref(false);

const rootRef = useTemplateRef('root');
const { run } = useAnime(() => rootRef.value);

async function loadPlaylists() {
  isLoading.value = true;
  errorMsg.value = '';
  try {
    playlists.value = await playlistApi.list();
  } catch (e) {
    errorMsg.value = e.message || String(e);
  } finally {
    isLoading.value = false;
  }
}

async function createPlaylist() {
  const name = newPlaylistName.value.trim();
  if (!name) return;
  creating.value = true;
  try {
    await playlistApi.create(name, newPlaylistDesc.value.trim() || null);
    showCreateDialog.value = false;
    newPlaylistName.value = '';
    newPlaylistDesc.value = '';
    await loadPlaylists();
  } catch (e) {
    alert(`创建歌单失败：${e.message || e}`);
  } finally {
    creating.value = false;
  }
}

function openPlaylist(id) {
  router.push(`/playlists/${id}`);
}

function goSearch() {
  router.push('/playlists/search');
}

function formatTime(ts) {
  if (!ts) return '';
  const d = new Date(ts);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

onMounted(async () => {
  await loadPlaylists();
  run(({ animate, stagger, presets }) => {
    animate('.page-header', { ...presets.fadeIn });
    animate('.playlist-card', {
      ...presets.fadeInUp,
      delay: stagger(50),
      onComplete: () => {
        rootRef.value?.querySelectorAll('.playlist-card').forEach((el) => {
          el.style.transform = '';
        });
      },
    });
  });
});
</script>

<template>
  <div class="playlists-page" ref="root">
    <div class="page-header">
      <div class="header-row">
        <div>
          <h1 class="page-title">歌单</h1>
          <p class="page-subtitle">共 {{ playlists.length }} 个歌单</p>
        </div>
        <div class="header-actions">
          <button class="btn btn-primary" @click="goSearch">
            <i class="bi bi-cloud-download"></i> 搜索在线歌单
          </button>
          <button class="btn btn-secondary" @click="showCreateDialog = true">
            <i class="bi bi-plus-lg"></i> 新建歌单
          </button>
        </div>
      </div>
    </div>

    <div v-if="isLoading" class="loading-state">
      <div class="spinner"></div>
    </div>

    <div v-else-if="errorMsg" class="error-state">
      <i class="bi bi-exclamation-triangle"></i>
      <p>{{ errorMsg }}</p>
    </div>

    <div v-else-if="playlists.length === 0" class="empty-state">
      <svg class="empty-state-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <path d="M9 18V5l12-2v13" />
        <circle cx="6" cy="18" r="3" />
        <circle cx="18" cy="16" r="3" />
      </svg>
      <h3 class="empty-state-title">暂无歌单</h3>
      <p class="empty-state-desc">新建歌单或从网易云搜索保存</p>
      <div class="empty-actions">
        <button class="btn btn-primary" @click="goSearch">搜索在线歌单</button>
        <button class="btn btn-secondary" @click="showCreateDialog = true">新建歌单</button>
      </div>
    </div>

    <div v-else class="playlists-grid">
      <div
        v-for="pl in playlists"
        :key="pl.id"
        class="playlist-card"
        @click="openPlaylist(pl.id)"
      >
        <div class="card-cover">
          <img
            v-if="pl.cover_url"
            :src="pl.cover_url"
            :alt="pl.name"
            loading="lazy"
            @error="(e) => e.target.style.display = 'none'"
          />
          <div v-if="!pl.cover_url" class="cover-placeholder">
            <i class="bi" :class="pl.source === 'netease' ? 'bi-cloud-arrow-down' : 'bi-music-note-list'"></i>
          </div>
          <div class="source-badge" :class="pl.source">
            <i class="bi" :class="pl.source === 'netease' ? 'bi-cloud' : 'bi-hdd'"></i>
          </div>
        </div>
        <div class="card-info">
          <h3 class="card-title" :title="pl.name">{{ pl.name }}</h3>
          <p class="card-meta">
            <span>{{ pl.song_ids.length }} 首</span>
            <span v-if="pl.creator" class="dot">·</span>
            <span v-if="pl.creator">{{ pl.creator }}</span>
          </p>
          <p class="card-time">{{ formatTime(pl.updated_at) }}</p>
        </div>
      </div>
    </div>

    <!-- 新建歌单对话框 -->
    <transition name="fade">
      <div v-if="showCreateDialog" class="dialog-mask" @click.self="showCreateDialog = false">
        <div class="dialog">
          <h3 class="dialog-title">新建歌单</h3>
          <div class="dialog-body">
            <label class="dialog-label">歌单名称</label>
            <input
              type="text"
              v-model="newPlaylistName"
              placeholder="请输入歌单名称"
              class="dialog-input"
              @keydown.enter="createPlaylist"
              autofocus
            />
            <label class="dialog-label">描述（可选）</label>
            <textarea
              v-model="newPlaylistDesc"
              placeholder="歌单描述"
              class="dialog-input"
              rows="3"
            ></textarea>
          </div>
          <div class="dialog-actions">
            <button class="btn btn-secondary" @click="showCreateDialog = false">取消</button>
            <button class="btn btn-primary" @click="createPlaylist" :disabled="creating || !newPlaylistName.trim()">
              {{ creating ? '创建中…' : '创建' }}
            </button>
          </div>
        </div>
      </div>
    </transition>
  </div>
</template>

<style scoped>
.playlists-page {
  max-width: 1200px;
  margin: 0 auto;
  padding-bottom: 32px;
}

.page-header {
  margin-bottom: 24px;
}

.header-row {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: 16px;
  flex-wrap: wrap;
}

.page-title {
  font-size: 26px;
  font-weight: 900;
  margin: 0 0 4px;
  color: var(--text-primary, #333);
}

.page-subtitle {
  font-size: 13px;
  color: var(--text-secondary, #666);
  margin: 0;
}

.header-actions {
  display: flex;
  gap: 8px;
}

.btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 8px 16px;
  font-size: 13px;
  font-weight: 500;
  border: 1px solid var(--border-color, #e8e8e8);
  border-radius: 8px;
  background: var(--bg-primary, #fff);
  color: var(--text-primary, #333);
  cursor: pointer;
  transition: all 0.2s ease;
}

.btn:hover:not(:disabled) {
  border-color: var(--primary-color, #0078d7);
  color: var(--primary-color, #0078d7);
}

.btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.btn-primary {
  background: var(--primary-color, #0078d7);
  border-color: var(--primary-color, #0078d7);
  color: #fff;
}

.btn-primary:hover:not(:disabled) {
  background: var(--primary-color-dark, #0066bf);
  color: #fff;
}

.loading-state,
.error-state,
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 60px 20px;
  text-align: center;
  color: var(--text-secondary, #666);
}

.empty-state-icon {
  width: 64px;
  height: 64px;
  margin-bottom: 16px;
  color: var(--text-tertiary, #999);
  opacity: 0.5;
}

.empty-state-title {
  font-size: 18px;
  font-weight: 700;
  margin: 0 0 6px;
  color: var(--text-primary, #333);
}

.empty-state-desc {
  font-size: 13px;
  margin: 0 0 16px;
}

.empty-actions {
  display: flex;
  gap: 8px;
}

.playlists-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
  gap: 20px;
}

.playlist-card {
  cursor: pointer;
  border-radius: 12px;
  background: var(--bg-secondary, #fff);
  border: 1px solid var(--border-light, #e8e8e8);
  overflow: hidden;
  transition: all 0.2s ease;
}

.playlist-card:hover {
  transform: translateY(-2px);
  border-color: var(--primary-color, #0078d7);
  box-shadow: 0 6px 20px rgba(0, 0, 0, 0.08);
}

.card-cover {
  position: relative;
  aspect-ratio: 1;
  background: var(--bg-hover, rgba(0, 0, 0, 0.04));
  overflow: hidden;
}

.card-cover img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.cover-placeholder {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 36px;
  color: var(--text-tertiary, #999);
}

.source-badge {
  position: absolute;
  top: 8px;
  right: 8px;
  width: 24px;
  height: 24px;
  border-radius: 50%;
  background: rgba(0, 0, 0, 0.6);
  color: #fff;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 12px;
  backdrop-filter: blur(8px);
}

.source-badge.netease {
  background: rgba(197, 0, 0, 0.85);
}

.card-info {
  padding: 10px 12px 12px;
}

.card-title {
  font-size: 14px;
  font-weight: 600;
  margin: 0 0 4px;
  color: var(--text-primary, #333);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.card-meta {
  font-size: 12px;
  color: var(--text-secondary, #666);
  margin: 0 0 2px;
  display: flex;
  align-items: center;
  gap: 4px;
}

.card-meta .dot {
  color: var(--text-tertiary, #999);
}

.card-time {
  font-size: 11px;
  color: var(--text-tertiary, #999);
  margin: 0;
}

.dialog-mask {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}

.dialog {
  width: 90%;
  max-width: 420px;
  background: var(--bg-secondary, #fff);
  border-radius: 12px;
  padding: 24px;
  box-shadow: 0 16px 48px rgba(0, 0, 0, 0.16);
}

.dialog-title {
  font-size: 18px;
  font-weight: 700;
  margin: 0 0 16px;
  color: var(--text-primary, #333);
}

.dialog-body {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-bottom: 20px;
}

.dialog-label {
  font-size: 12px;
  color: var(--text-secondary, #666);
  font-weight: 500;
}

.dialog-input {
  width: 100%;
  padding: 10px 12px;
  font-size: 14px;
  border: 1px solid var(--border-color, #e8e8e8);
  border-radius: 8px;
  background: var(--bg-primary, #fff);
  color: var(--text-primary, #333);
  outline: none;
  font-family: inherit;
  box-sizing: border-box;
}

.dialog-input:focus {
  border-color: var(--primary-color, #0078d7);
}

.dialog-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}

.spinner {
  width: 32px;
  height: 32px;
  border: 3px solid var(--border-light, #e8e8e8);
  border-top-color: var(--primary-color, #0078d7);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.2s;
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}

@media (max-width: 600px) {
  .playlists-grid {
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: 12px;
  }

  .header-actions {
    width: 100%;
  }

  .btn {
    flex: 1;
    justify-content: center;
  }
}
</style>
