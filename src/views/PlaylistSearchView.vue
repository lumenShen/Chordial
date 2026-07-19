<script setup>
/**
 * PlaylistSearchView — 在线搜索网易云歌单并保存到本地库
 *
 * 流程：
 *  1. 用户输入关键词，调用 netease_search_playlists 拉取在线歌单列表
 *  2. 点击歌单卡片预览详情（曲目数 / 创建者 / 描述）
 *  3. "保存到本地" 按钮 → netease_save_playlist_to_library
 *     后端会自动把歌单中每首歌加入音乐库（去重），并创建本地 Playlist 记录
 */
import { ref, shallowRef, computed, useTemplateRef } from 'vue';
import { useRouter } from 'vue-router';
import { useAnime } from '@/composables/useAnime.js';
import * as neteaseApi from '@/api/musicSource/netease.js';

const router = useRouter();

const query = ref('');
const limit = ref(30);
const offset = ref(0);

// shallowRef：业务对象数组，避免深代理
const results = shallowRef([]);
const totalCount = ref(0);
const isLoading = ref(false);
const errorMsg = ref('');

// 预览弹窗
const previewDetail = ref(null);
const previewLoading = ref(false);
const previewTracks = shallowRef([]);
const saving = ref(false);

// 已保存的歌单 ID（本地去重提示）
const savedIds = ref(new Set());

// Toast
const toast = ref(null);
let toastTimer = null;
function showToast(msg, type = 'info', duration = 2500) {
  toast.value = { msg, type };
  if (toastTimer) clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { toast.value = null; }, duration);
}

const rootRef = useTemplateRef('root');
const { run } = useAnime(() => rootRef.value);

// ── 搜索 ──────────────────────────────────────────────
let debounceTimer = null;

async function executeSearch() {
  const q = query.value.trim();
  if (!q) return;
  isLoading.value = true;
  errorMsg.value = '';
  try {
    const list = await neteaseApi.searchPlaylists(q, limit.value, offset.value);
    results.value = list || [];
    totalCount.value = (list || []).length;
    run(({ animate, stagger, presets }) => {
      animate('.result-card', {
        ...presets.fadeInUp,
        delay: stagger(40),
        onComplete: () => {
          rootRef.value?.querySelectorAll('.result-card').forEach((el) => {
            el.style.transform = '';
          });
        },
      });
    });
  } catch (e) {
    errorMsg.value = e.message || String(e);
    results.value = [];
  } finally {
    isLoading.value = false;
  }
}

function onInput() {
  if (debounceTimer) clearTimeout(debounceTimer);
  debounceTimer = setTimeout(executeSearch, 350);
}

function onSubmit() {
  if (debounceTimer) {
    clearTimeout(debounceTimer);
    debounceTimer = null;
  }
  executeSearch();
}

function clearSearch() {
  query.value = '';
  results.value = [];
  totalCount.value = 0;
  errorMsg.value = '';
}

// ── 预览 ──────────────────────────────────────────────
async function previewPlaylist(item) {
  previewLoading.value = true;
  previewDetail.value = item;
  previewTracks.value = [];
  try {
    const data = await neteaseApi.getPlaylistFull(item.id);
    if (data) {
      previewDetail.value = {
        ...item,
        ...(data.detail || {}),
      };
      previewTracks.value = data.tracks || [];
    }
  } catch (e) {
    showToast(`加载详情失败：${e.message || e}`, 'error');
  } finally {
    previewLoading.value = false;
  }
}

function closePreview() {
  previewDetail.value = null;
  previewTracks.value = [];
}

// ── 保存到本地 ────────────────────────────────────────
async function saveToLocal() {
  if (!previewDetail.value) return;
  saving.value = true;
  try {
    const playlist = await neteaseApi.savePlaylistToLibrary(previewDetail.value.id);
    savedIds.value.add(previewDetail.value.id);
    showToast(`已保存到本地：${playlist.name || previewDetail.value.name}`, 'success');
    closePreview();
  } catch (e) {
    showToast(`保存失败：${e.message || e}`, 'error');
  } finally {
    saving.value = false;
  }
}

// ── 跳转本地歌单 ─────────────────────────────────────
function goLocalPlaylists() {
  router.push('/playlists');
}

// ── 工具 ─────────────────────────────────────────────
function formatPlayCount(n) {
  if (n == null) return '';
  if (n >= 1e8) return (n / 1e8).toFixed(1) + '亿';
  if (n >= 1e4) return (n / 1e4).toFixed(1) + '万';
  return String(n);
}

const hasResults = computed(() => results.value.length > 0);
</script>

<template>
  <div class="playlist-search" ref="root">
    <div class="page-header">
      <button class="back-btn" @click="goLocalPlaylists">
        <i class="bi bi-arrow-left"></i>
      </button>
      <div>
        <h1 class="page-title">搜索网易云歌单</h1>
        <p class="page-subtitle">搜索在线歌单并保存到本地音乐库</p>
      </div>
    </div>

    <!-- 搜索框 -->
    <div class="search-row">
      <div class="search-box">
        <i class="bi bi-search search-icon"></i>
        <input
          v-model="query"
          type="text"
          placeholder="输入歌单名、关键词..."
          @input="onInput"
          @keyup.enter="onSubmit"
        />
        <button v-if="query" class="clear-btn" @click="clearSearch">
          <i class="bi bi-x-lg"></i>
        </button>
      </div>
      <select v-model.number="limit" class="limit-select" @change="executeSearch">
        <option :value="20">20</option>
        <option :value="30">30</option>
        <option :value="50">50</option>
      </select>
    </div>

    <!-- 错误 -->
    <div v-if="errorMsg" class="error-state">
      <i class="bi bi-exclamation-triangle"></i>
      <span>{{ errorMsg }}</span>
    </div>

    <!-- 加载 -->
    <div v-else-if="isLoading" class="loading-state">
      <div class="spinner"></div>
      <p>搜索中…</p>
    </div>

    <!-- 空状态：未搜索 -->
    <div v-else-if="!query && !hasResults" class="empty-state">
      <i class="bi bi-cloud-search empty-icon"></i>
      <h3 class="empty-title">输入关键词开始搜索</h3>
      <p class="empty-desc">需要先在「设置 → 网易云音乐来源」配置 API 服务器</p>
      <router-link to="/settings/netease" class="btn btn-primary">去配置</router-link>
    </div>

    <!-- 空状态：无结果 -->
    <div v-else-if="!hasResults" class="empty-state">
      <i class="bi bi-emoji-frown empty-icon"></i>
      <h3 class="empty-title">未找到 "{{ query }}" 的相关歌单</h3>
      <p class="empty-desc">试试其他关键词</p>
    </div>

    <!-- 结果列表 -->
    <div v-else class="results-grid">
      <div
        v-for="item in results"
        :key="item.id"
        class="result-card"
        @click="previewPlaylist(item)"
      >
        <div class="card-cover">
          <img
            v-if="item.coverImgUrl"
            :src="item.coverImgUrl"
            :alt="item.name"
            loading="lazy"
            @error="(e) => e.target.style.display = 'none'"
          />
          <div v-else class="cover-placeholder">
            <i class="bi bi-music-note-list"></i>
          </div>
          <div v-if="savedIds.has(item.id)" class="saved-badge" title="已保存到本地">
            <i class="bi bi-check-circle-fill"></i>
          </div>
        </div>
        <div class="card-info">
          <h3 class="card-title" :title="item.name">{{ item.name }}</h3>
          <p class="card-meta">
            <span v-if="item.creator">{{ item.creator.nickname }}</span>
            <span class="dot">·</span>
            <span>{{ item.trackCount }} 首</span>
            <template v-if="item.playCount">
              <span class="dot">·</span>
              <span>{{ formatPlayCount(item.playCount) }} 次播放</span>
            </template>
          </p>
          <p v-if="item.description" class="card-desc">{{ item.description }}</p>
        </div>
      </div>
    </div>

    <!-- 预览弹窗 -->
    <transition name="fade">
      <div v-if="previewDetail" class="dialog-mask" @click.self="closePreview">
        <div class="preview-dialog">
          <div class="preview-header">
            <div class="preview-cover">
              <img
                v-if="previewDetail.coverImgUrl"
                :src="previewDetail.coverImgUrl"
                :alt="previewDetail.name"
                @error="(e) => e.target.style.display = 'none'"
              />
              <div v-else class="cover-placeholder">
                <i class="bi bi-music-note-list"></i>
              </div>
            </div>
            <div class="preview-meta">
              <h2 class="preview-title">{{ previewDetail.name }}</h2>
              <p class="preview-sub" v-if="previewDetail.creator">
                创建者：{{ previewDetail.creator.nickname }}
              </p>
              <p class="preview-sub">
                曲目数：{{ previewTracks.length }}
                <span v-if="previewDetail.trackCount"> / {{ previewDetail.trackCount }}</span>
              </p>
              <div class="preview-tags" v-if="previewDetail.tags && previewDetail.tags.length">
                <span v-for="t in previewDetail.tags" :key="t" class="tag">{{ t }}</span>
              </div>
            </div>
            <button class="close-btn" @click="closePreview">
              <i class="bi bi-x-lg"></i>
            </button>
          </div>

          <p v-if="previewDetail.description" class="preview-desc">
            {{ previewDetail.description }}
          </p>

          <div class="preview-actions">
            <button class="btn btn-secondary" @click="closePreview">取消</button>
            <button
              class="btn btn-primary"
              :disabled="saving || previewLoading"
              @click="saveToLocal"
            >
              <i class="bi bi-cloud-download"></i>
              {{ saving ? '保存中…' : '保存到本地音乐库' }}
            </button>
          </div>

          <div class="preview-tracks">
            <div v-if="previewLoading" class="tracks-loading">
              <div class="spinner-small"></div> 加载曲目中…
            </div>
            <div v-else-if="previewTracks.length === 0" class="tracks-empty">
              暂无曲目
            </div>
            <div v-else class="track-list-mini">
              <div
                v-for="(track, idx) in previewTracks"
                :key="track.id"
                class="track-row"
              >
                <span class="track-idx">{{ idx + 1 }}</span>
                <div class="track-info">
                  <div class="track-name" :title="track.name">{{ track.name }}</div>
                  <div class="track-artist">
                    {{ (track.ar || []).map((a) => a.name).join(' / ') }}
                    <span v-if="track.al && track.al.name"> - {{ track.al.name }}</span>
                  </div>
                </div>
                <span class="track-dur">
                  {{ Math.floor((track.dt || 0) / 60000) }}:{{
                    String(Math.floor(((track.dt || 0) % 60000) / 1000)).padStart(2, '0')
                  }}
                </span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </transition>

    <!-- Toast -->
    <transition name="fade">
      <div v-if="toast" class="toast" :class="toast.type">
        <i class="bi" :class="{
          success: 'bi-check-circle-fill',
          error: 'bi-x-circle-fill',
          info: 'bi-info-circle-fill',
        }[toast.type]"></i>
        <span>{{ toast.msg }}</span>
      </div>
    </transition>
  </div>
</template>

<style scoped>
.playlist-search {
  max-width: 1200px;
  margin: 0 auto;
  padding-bottom: 80px;
}

.page-header {
  display: flex;
  align-items: center;
  gap: 16px;
  margin-bottom: 24px;
}

.back-btn {
  width: 36px;
  height: 36px;
  border: 1px solid var(--border-light, #e8e8e8);
  background: var(--bg-secondary, #fff);
  border-radius: 50%;
  color: var(--text-secondary, #666);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: all 0.2s ease;
}

.back-btn:hover {
  border-color: var(--primary-color, #0078d7);
  color: var(--primary-color, #0078d7);
}

.page-title {
  font-size: 24px;
  font-weight: 800;
  margin: 0 0 4px;
  color: var(--text-primary, #333);
}

.page-subtitle {
  font-size: 13px;
  color: var(--text-secondary, #666);
  margin: 0;
}

.search-row {
  display: flex;
  gap: 12px;
  margin-bottom: 24px;
}

.search-box {
  position: relative;
  flex: 1;
  display: flex;
  align-items: center;
}

.search-icon {
  position: absolute;
  left: 14px;
  font-size: 16px;
  color: var(--text-tertiary, #999);
  pointer-events: none;
}

.search-box input {
  width: 100%;
  height: 44px;
  padding: 0 44px 0 44px;
  border: 1px solid var(--border-light, #e8e8e8);
  border-radius: 22px;
  background: var(--bg-secondary, #fff);
  color: var(--text-primary, #333);
  font-size: 15px;
  outline: none;
  transition: all 0.2s ease;
  box-sizing: border-box;
}

.search-box input:focus {
  border-color: var(--primary-color, #0078d7);
  box-shadow: 0 0 0 4px var(--primary-light, rgba(0, 120, 215, 0.1));
}

.clear-btn {
  position: absolute;
  right: 10px;
  width: 28px;
  height: 28px;
  border: none;
  border-radius: 50%;
  background: var(--bg-hover, rgba(0, 0, 0, 0.04));
  color: var(--text-secondary, #666);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 11px;
}

.limit-select {
  height: 44px;
  padding: 0 12px;
  border: 1px solid var(--border-light, #e8e8e8);
  border-radius: 22px;
  background: var(--bg-secondary, #fff);
  color: var(--text-primary, #333);
  font-size: 14px;
  cursor: pointer;
  outline: none;
}

.error-state,
.loading-state,
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 60px 20px;
  text-align: center;
  color: var(--text-secondary, #666);
  gap: 8px;
}

.empty-icon {
  font-size: 56px;
  color: var(--text-tertiary, #999);
  margin-bottom: 8px;
  opacity: 0.5;
}

.empty-title {
  font-size: 18px;
  font-weight: 700;
  color: var(--text-primary, #333);
  margin: 0;
}

.empty-desc {
  font-size: 13px;
  margin: 0 0 16px;
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
  text-decoration: none;
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

.spinner {
  width: 32px;
  height: 32px;
  border: 3px solid var(--border-light, #e8e8e8);
  border-top-color: var(--primary-color, #0078d7);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

.spinner-small {
  display: inline-block;
  width: 14px;
  height: 14px;
  border: 2px solid var(--border-light, #e8e8e8);
  border-top-color: var(--primary-color, #0078d7);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
  vertical-align: middle;
  margin-right: 6px;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

.results-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  gap: 20px;
}

.result-card {
  cursor: pointer;
  border-radius: 12px;
  background: var(--bg-secondary, #fff);
  border: 1px solid var(--border-light, #e8e8e8);
  overflow: hidden;
  transition: all 0.2s ease;
}

.result-card:hover {
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

.saved-badge {
  position: absolute;
  top: 8px;
  right: 8px;
  width: 24px;
  height: 24px;
  border-radius: 50%;
  background: rgba(40, 167, 69, 0.9);
  color: #fff;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 12px;
  backdrop-filter: blur(8px);
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
  flex-wrap: wrap;
  gap: 4px;
}

.card-meta .dot {
  color: var(--text-tertiary, #999);
}

.card-desc {
  font-size: 11px;
  color: var(--text-tertiary, #999);
  margin: 4px 0 0;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

/* 预览弹窗 */
.dialog-mask {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.6);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
  padding: 20px;
}

.preview-dialog {
  width: 100%;
  max-width: 720px;
  max-height: 90vh;
  background: var(--bg-secondary, #fff);
  border-radius: 16px;
  padding: 24px;
  box-shadow: 0 16px 48px rgba(0, 0, 0, 0.2);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.preview-header {
  display: flex;
  gap: 16px;
  margin-bottom: 16px;
}

.preview-cover {
  width: 120px;
  height: 120px;
  border-radius: 12px;
  overflow: hidden;
  flex-shrink: 0;
  background: var(--bg-hover, rgba(0, 0, 0, 0.04));
}

.preview-cover img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.preview-meta {
  flex: 1;
  min-width: 0;
}

.preview-title {
  font-size: 20px;
  font-weight: 800;
  margin: 0 0 6px;
  color: var(--text-primary, #333);
  word-break: break-word;
}

.preview-sub {
  font-size: 12px;
  color: var(--text-secondary, #666);
  margin: 0 0 4px;
}

.preview-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  margin-top: 8px;
}

.tag {
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 10px;
  background: var(--primary-light, rgba(0, 120, 215, 0.1));
  color: var(--primary-color, #0078d7);
}

.close-btn {
  width: 32px;
  height: 32px;
  border: none;
  background: transparent;
  color: var(--text-secondary, #666);
  cursor: pointer;
  border-radius: 50%;
  font-size: 14px;
  transition: all 0.2s ease;
  flex-shrink: 0;
}

.close-btn:hover {
  background: var(--bg-hover, rgba(0, 0, 0, 0.04));
  color: var(--text-primary, #333);
}

.preview-desc {
  font-size: 13px;
  color: var(--text-secondary, #666);
  margin: 0 0 16px;
  line-height: 1.6;
  word-break: break-word;
}

.preview-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding-bottom: 12px;
  border-bottom: 1px solid var(--border-light, #e8e8e8);
  margin-bottom: 12px;
}

.preview-tracks {
  flex: 1;
  overflow-y: auto;
  min-height: 120px;
  max-height: 40vh;
}

.tracks-loading,
.tracks-empty {
  padding: 24px;
  text-align: center;
  color: var(--text-secondary, #666);
  font-size: 13px;
}

.track-list-mini {
  display: flex;
  flex-direction: column;
}

.track-row {
  display: grid;
  grid-template-columns: 32px 1fr 60px;
  gap: 10px;
  padding: 8px 4px;
  align-items: center;
  border-bottom: 1px solid var(--border-light, #eee);
  font-size: 13px;
}

.track-row:last-child {
  border-bottom: none;
}

.track-idx {
  color: var(--text-tertiary, #999);
  text-align: center;
}

.track-info {
  min-width: 0;
}

.track-name {
  font-weight: 500;
  color: var(--text-primary, #333);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.track-artist {
  font-size: 11px;
  color: var(--text-secondary, #666);
  margin-top: 2px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.track-dur {
  text-align: right;
  color: var(--text-tertiary, #999);
  font-variant-numeric: tabular-nums;
  font-size: 12px;
}

/* Toast */
.toast {
  position: fixed;
  bottom: 32px;
  left: 50%;
  transform: translateX(-50%);
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px 20px;
  background: var(--bg-secondary, #fff);
  border: 1px solid var(--border-light, #e8e8e8);
  border-radius: 12px;
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.12);
  font-size: 13px;
  z-index: 1100;
  max-width: 90vw;
}

.toast.success {
  color: #28a745;
  border-color: #28a745;
}

.toast.error {
  color: #dc3545;
  border-color: #dc3545;
}

.toast.info {
  color: var(--primary-color, #0078d7);
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
  .results-grid {
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 12px;
  }

  .preview-header {
    flex-direction: column;
    gap: 12px;
  }

  .preview-cover {
    width: 100%;
    aspect-ratio: 1;
    height: auto;
  }
}
</style>
