<script setup>
/**
 * PlaylistDetailView — 查看 / 编辑本地歌单
 *
 * 功能：
 *  - 显示歌单元信息（封面 / 名称 / 描述 / 来源 / 创建者 / 曲目数）
 *  - 列出歌单内所有曲目（复用 TrackList 组件）
 *  - 编辑：名称、描述、标签
 *  - 删除歌单
 *  - 移除单曲、调整顺序
 */
import { ref, shallowRef, computed, onMounted } from 'vue';
import { useRouter, useRoute } from 'vue-router';
import * as playlistApi from '@/api/musicSource/playlists.js';
import TrackList from '@/components/common/TrackList.vue';
import BatchDownloadDialog from '@/components/common/BatchDownloadDialog.vue';
import PlayerStore from '@/stores/player/index.js';
import { useDownloadManager } from '@/stores/downloadManager.js';
import { formatDuration } from '@/utils/format.js';

const { startBatch: startBatchDownload } = useDownloadManager();

const router = useRouter();
const route = useRoute();

const playlistId = computed(() => route.params.id);

const playlist = ref(null);
const songs = shallowRef([]);
const isLoading = ref(false);
const errorMsg = ref('');

// 编辑模式
const isEditing = ref(false);
const editForm = ref({
  name: '',
  description: '',
  tags: [],
});
const tagInput = ref('');
const saving = ref(false);

// 删除确认
const pendingDelete = ref(false);
let deleteTimer = null;

// 拖拽重排
const dragIndex = ref(null);
const dragOverIndex = ref(null);

// Toast
const toast = ref(null);
let toastTimer = null;
function showToast(msg, type = 'info', duration = 2500) {
  toast.value = { msg, type };
  if (toastTimer) clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { toast.value = null; }, duration);
}

// ── 加载 ──────────────────────────────────────────────
async function loadPlaylist() {
  isLoading.value = true;
  errorMsg.value = '';
  try {
    const data = await playlistApi.get(playlistId.value);
    if (!data) {
      errorMsg.value = '歌单不存在';
      playlist.value = null;
      songs.value = [];
      return;
    }
    playlist.value = data.playlist;
    songs.value = data.songs || [];
  } catch (e) {
    errorMsg.value = e.message || String(e);
  } finally {
    isLoading.value = false;
  }
}

// ── 编辑 ──────────────────────────────────────────────
function startEdit() {
  if (!playlist.value) return;
  editForm.value = {
    name: playlist.value.name || '',
    description: playlist.value.description || '',
    tags: [...(playlist.value.tags || [])],
  };
  tagInput.value = '';
  isEditing.value = true;
}

function cancelEdit() {
  isEditing.value = false;
}

async function saveEdit() {
  if (!playlist.value) return;
  const name = editForm.value.name.trim();
  if (!name) {
    showToast('歌单名称不能为空', 'error');
    return;
  }
  saving.value = true;
  try {
    const updated = {
      ...playlist.value,
      name,
      description: editForm.value.description.trim() || null,
      tags: editForm.value.tags,
    };
    await playlistApi.update(updated);
    playlist.value = updated;
    isEditing.value = false;
    showToast('已保存', 'success');
  } catch (e) {
    showToast(`保存失败：${e.message || e}`, 'error');
  } finally {
    saving.value = false;
  }
}

function addTag() {
  const t = tagInput.value.trim();
  if (!t) return;
  if (!editForm.value.tags.includes(t)) {
    editForm.value.tags.push(t);
  }
  tagInput.value = '';
}

function removeTag(t) {
  editForm.value.tags = editForm.value.tags.filter((x) => x !== t);
}

// ── 删除歌单 ──────────────────────────────────────────
function confirmDelete() {
  if (pendingDelete.value) {
    // 二次确认 → 执行删除
    if (deleteTimer) {
      clearTimeout(deleteTimer);
      deleteTimer = null;
    }
    pendingDelete.value = false;
    doDelete();
    return;
  }
  pendingDelete.value = true;
  deleteTimer = setTimeout(() => {
    pendingDelete.value = false;
  }, 3000);
}

async function doDelete() {
  try {
    await playlistApi.removePlaylist(playlistId.value);
    showToast('歌单已删除', 'success');
    setTimeout(() => router.push('/playlists'), 400);
  } catch (e) {
    showToast(`删除失败：${e.message || e}`, 'error');
  }
}

// ── 曲目操作 ──────────────────────────────────────────
async function removeSong(songId) {
  try {
    await playlistApi.removeSong(playlistId.value, songId);
    songs.value = songs.value.filter((s) => s.id !== songId);
    if (playlist.value) {
      playlist.value = {
        ...playlist.value,
        song_ids: (playlist.value.song_ids || []).filter((id) => id !== songId),
      };
    }
    showToast('已从歌单移除', 'success');
  } catch (e) {
    showToast(`移除失败：${e.message || e}`, 'error');
  }
}

// ── 拖拽重排 ──────────────────────────────────────────
function onDragStart(idx) {
  dragIndex.value = idx;
}

function onDragOver(idx, e) {
  e.preventDefault();
  dragOverIndex.value = idx;
}

function onDragLeave() {
  dragOverIndex.value = null;
}

async function onDrop(idx) {
  const from = dragIndex.value;
  const to = idx;
  dragIndex.value = null;
  dragOverIndex.value = null;
  if (from == null || from === to) return;
  const arr = [...songs.value];
  const [moved] = arr.splice(from, 1);
  arr.splice(to, 0, moved);
  songs.value = arr;
  const newOrder = arr.map((s) => s.id);
  try {
    await playlistApi.reorderSongs(playlistId.value, newOrder);
    if (playlist.value) {
      playlist.value = { ...playlist.value, song_ids: newOrder };
    }
    showToast('顺序已更新', 'success');
  } catch (e) {
    showToast(`调整失败：${e.message || e}`, 'error');
    // 回滚
    await loadPlaylist();
  }
}

// ── 播放 ──────────────────────────────────────────────
function playAll() {
  if (songs.value.length === 0) return;
  PlayerStore.play(songs.value[0], songs.value);
}

function playSong(track) {
  PlayerStore.play(track, songs.value);
}

function openTrack(track) {
  router.push(`/track/${track.id}`);
}

// ── 批量下载 ─────────────────────────────────────────
// 弹出配置对话框 → 用户选择目标音质与去重策略 → 启动后台下载。
// 对话框关闭/确认由 BatchDownloadDialog 组件 emit 控制。
const batchDialog = ref({
  visible: false,
  items: [],
  starting: false,
});

function openBatchDialog() {
  if (songs.value.length === 0) return;
  const items = [];
  for (const s of songs.value) {
    if (!s?.sourceIds) continue;
    const sid = s.sourceIds.find((x) => x?.sourceName === 'netease');
    if (!sid?.entityId) continue;
    items.push({
      songEntityId: sid.entityId,
      title: s.title || '未知',
      artist: s.getDisplayArtist ? s.getDisplayArtist() : (s.artist || ''),
      level: null, // 由对话框的 targetLevel 覆盖
    });
  }
  if (items.length === 0) {
    showToast('当前歌单没有可下载的网易云曲目', 'info');
    return;
  }
  batchDialog.value.items = items;
  batchDialog.value.visible = true;
}

async function onBatchConfirm({ targetLevel, dedupStrategy }) {
  batchDialog.value.starting = true;
  try {
    const { total } = await startBatchDownload(batchDialog.value.items, {
      targetLevel,
      dedupStrategy,
    });
    showToast(`已开始后台下载 ${total} 首曲目`, 'success');
    batchDialog.value.visible = false;
  } catch (e) {
    showToast(`批量下载启动失败：${e.message || e}`, 'error');
  } finally {
    batchDialog.value.starting = false;
  }
}

// ── 工具 ─────────────────────────────────────────────
function formatTime(ts) {
  if (!ts) return '';
  const d = new Date(ts);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

const sourceLabel = computed(() => {
  if (!playlist.value) return '';
  return playlist.value.source === 'netease' ? '网易云' : '本地';
});

onMounted(async () => {
  await loadPlaylist();
});
</script>

<template>
  <div class="playlist-detail page-container">
    <!-- 返回 -->
    <button class="back-btn" @click="router.push('/playlists')">
      <HIcon name="arrow-left" /> 返回歌单
    </button>

    <!-- 加载 -->
    <div v-if="isLoading" class="loading-state">
      <div class="spinner"></div>
    </div>

    <!-- 错误 -->
    <div v-else-if="errorMsg" class="error-state">
      <HIcon name="exclamation-triangle" />
      <p>{{ errorMsg }}</p>
      <button class="btn btn-primary" @click="router.push('/playlists')">返回歌单列表</button>
    </div>

    <template v-else-if="playlist">
      <!-- 头部 -->
      <header class="playlist-header">
        <div class="header-cover">
          <img
            v-if="playlist.cover_url"
            :src="playlist.cover_url"
            :alt="playlist.name"
            @error="(e) => e.target.style.display = 'none'"
          />
          <div v-else class="cover-placeholder">
            <i class="bi" :class="playlist.source === 'netease' ? 'bi-cloud' : 'bi-music-note-list'"></i>
          </div>
        </div>

        <div class="header-info">
          <div class="header-meta-row">
            <span class="source-badge" :class="playlist.source">{{ sourceLabel }}</span>
            <span class="meta-text">歌单</span>
          </div>

          <!-- 非编辑模式 -->
          <template v-if="!isEditing">
            <h1 class="header-title">{{ playlist.name }}</h1>
            <p v-if="playlist.description" class="header-desc">{{ playlist.description }}</p>
            <div class="header-tags" v-if="playlist.tags && playlist.tags.length">
              <span v-for="t in playlist.tags" :key="t" class="tag">{{ t }}</span>
            </div>
            <div class="header-stats">
              <span v-if="playlist.creator">
                <HIcon name="person" /> {{ playlist.creator }}
              </span>
              <span>
                <HIcon name="music-note-beamed" /> {{ songs.length }} 首
              </span>
              <span v-if="playlist.created_at">
                <HIcon name="calendar" /> {{ formatTime(playlist.created_at) }}
              </span>
            </div>
          </template>

          <!-- 编辑模式 -->
          <template v-else>
            <label class="edit-label">歌单名称</label>
            <input
              v-model="editForm.name"
              type="text"
              class="edit-input"
              placeholder="歌单名称"
            />
            <label class="edit-label">描述</label>
            <textarea
              v-model="editForm.description"
              class="edit-textarea"
              rows="3"
              placeholder="歌单描述（可选）"
            ></textarea>
            <label class="edit-label">标签</label>
            <div class="tag-editor">
              <div class="tag-list">
                <span v-for="t in editForm.tags" :key="t" class="tag removable">
                  {{ t }}
                  <button class="tag-remove" @click="removeTag(t)">×</button>
                </span>
              </div>
              <div class="tag-input-row">
                <input
                  v-model="tagInput"
                  type="text"
                  class="edit-input"
                  placeholder="添加标签后回车"
                  @keydown.enter.prevent="addTag"
                />
                <button class="btn btn-secondary btn-sm" @click="addTag">添加</button>
              </div>
            </div>
          </template>
        </div>

        <!-- 操作按钮 -->
        <div class="header-actions">
          <template v-if="!isEditing">
            <button
              class="btn btn-primary"
              :disabled="songs.length === 0"
              @click="playAll"
            >
              <HIcon name="play-fill" /> 播放全部
            </button>
            <button
              class="btn btn-secondary"
              :disabled="songs.length === 0"
              @click="openBatchDialog"
              title="将网易云曲目批量下载到本地（后台静默执行）"
            >
              <HIcon name="cloud-arrow-down" /> 批量下载
            </button>
            <button class="btn btn-secondary" @click="startEdit">
              <HIcon name="pencil" /> 编辑
            </button>
            <button
              class="btn btn-danger"
              :class="{ confirming: pendingDelete }"
              @click="confirmDelete"
            >
              {{ pendingDelete ? '确认删除' : '删除' }}
            </button>
          </template>
          <template v-else>
            <button class="btn btn-secondary" @click="cancelEdit" :disabled="saving">取消</button>
            <button class="btn btn-primary" @click="saveEdit" :disabled="saving">
              {{ saving ? '保存中…' : '保存' }}
            </button>
          </template>
        </div>
      </header>

      <!-- 曲目列表 -->
      <section class="tracks-section">
        <div class="tracks-header">
          <h2 class="tracks-title">曲目</h2>
          <span class="tracks-count">{{ songs.length }} 首</span>
        </div>

        <div v-if="songs.length === 0" class="empty-tracks">
          <HIcon name="music-note" class="empty-icon" />
          <p>歌单为空</p>
        </div>

        <!-- 自定义可拖拽列表（编辑模式下） -->
        <div v-else-if="isEditing" class="editable-tracks">
          <div
            v-for="(song, idx) in songs"
            :key="song.id"
            class="editable-row"
            :class="{
              'drag-from': dragIndex === idx,
              'drag-over': dragOverIndex === idx && dragIndex !== idx,
            }"
            draggable="true"
            @dragstart="onDragStart(idx)"
            @dragover="onDragOver(idx, $event)"
            @dragleave="onDragLeave"
            @drop="onDrop(idx)"
            @dragend="dragIndex = null; dragOverIndex = null"
          >
            <div class="row-handle" title="拖拽调整顺序">
              <HIcon name="grip-vertical" />
            </div>
            <span class="row-idx">{{ idx + 1 }}</span>
            <div class="row-info">
              <div class="row-title" :title="song.title">{{ song.title }}</div>
              <div class="row-artist">{{ song.getDisplayArtist ? song.getDisplayArtist() : (song.artist || '未知歌手') }}</div>
            </div>
            <span class="row-dur">{{ formatDuration(song.duration) }}</span>
            <button class="row-remove" @click="removeSong(song.id)" title="从歌单移除">
              <HIcon name="x-lg" />
            </button>
          </div>
        </div>

        <!-- 只读模式：使用 TrackList -->
        <!-- mode="page"：复用外层 .content-wrapper 滚动，避免双层嵌套滚动 -->
        <TrackList
          v-else
          :tracks="songs"
          :virtual-scroll="true"
          mode="page"
          :show-match-count="true"
          @select="openTrack"
          @play="playSong"
        />
      </section>
    </template>

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

    <!-- 批量下载配置对话框 -->
    <BatchDownloadDialog
      v-model="batchDialog.visible"
      :total="batchDialog.items.length"
      :loading="batchDialog.starting"
      @confirm="onBatchConfirm"
    />
  </div>
</template>

<style scoped>
.playlist-detail {
  /* max-width 和 margin 由 .page-container 提供 */
}

.back-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  margin-bottom: 16px;
  border: 1px solid var(--border-light, #e8e8e8);
  background: transparent;
  color: var(--text-secondary, #666);
  border-radius: 8px;
  cursor: pointer;
  font-size: 13px;
  transition: all 0.2s ease;
}

.back-btn:hover {
  border-color: var(--primary-color);
  color: var(--primary-color);
}

.loading-state,
.error-state,
.empty-tracks {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 60px 20px;
  text-align: center;
  color: var(--text-secondary, #666);
  gap: 12px;
}

.spinner {
  width: 32px;
  height: 32px;
  border: 3px solid var(--border-light, #e8e8e8);
  border-top-color: var(--primary-color);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

/* 头部 */
.playlist-header {
  display: grid;
  grid-template-columns: 220px 1fr auto;
  gap: 24px;
  align-items: flex-start;
  padding: 24px;
  background: var(--bg-secondary, #fff);
  border-radius: 16px;
  border: 1px solid var(--border-light, #e8e8e8);
  margin-bottom: 24px;
}

.header-cover {
  width: 220px;
  height: 220px;
  border-radius: 12px;
  overflow: hidden;
  background: var(--bg-hover, rgba(0, 0, 0, 0.04));
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.08);
}

.header-cover img {
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
  font-size: 64px;
  color: var(--text-tertiary, #999);
}

.header-info {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.header-meta-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--text-tertiary, #999);
}

.source-badge {
  padding: 2px 8px;
  border-radius: 10px;
  font-size: 11px;
  font-weight: 600;
  background: var(--bg-hover, rgba(0, 0, 0, 0.04));
  color: var(--text-secondary, #666);
}

.source-badge.netease {
  background: rgba(197, 0, 0, 0.1);
  color: #c50000;
}

.header-title {
  font-size: 28px;
  font-weight: 800;
  margin: 4px 0 0;
  color: var(--text-primary, #333);
  word-break: break-word;
}

.header-desc {
  font-size: 13px;
  color: var(--text-secondary, #666);
  margin: 4px 0 0;
  line-height: 1.6;
  word-break: break-word;
}

.header-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  margin-top: 6px;
}

.tag {
  font-size: 11px;
  padding: 3px 10px;
  border-radius: 12px;
  background: var(--primary-light);
  color: var(--primary-color);
}

.tag.removable {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  background: var(--bg-hover, rgba(0, 0, 0, 0.04));
  color: var(--text-secondary, #666);
}

.tag-remove {
  border: none;
  background: transparent;
  color: inherit;
  cursor: pointer;
  font-size: 14px;
  line-height: 1;
  padding: 0;
}

.header-stats {
  display: flex;
  flex-wrap: wrap;
  gap: 16px;
  margin-top: 8px;
  font-size: 12px;
  color: var(--text-tertiary, #999);
}

.header-stats i {
  margin-right: 4px;
}

.header-actions {
  display: flex;
  flex-direction: column;
  gap: 8px;
  align-items: stretch;
  min-width: 100px;
}

.btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 8px 14px;
  font-size: 13px;
  font-weight: 500;
  border: 1px solid var(--border-color, #e8e8e8);
  border-radius: 8px;
  background: var(--bg-primary, #fff);
  color: var(--text-primary, #333);
  cursor: pointer;
  transition: all 0.2s ease;
  white-space: nowrap;
}

.btn:hover:not(:disabled) {
  border-color: var(--primary-color);
  color: var(--primary-color);
}

.btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.btn-primary {
  background: var(--primary-color);
  border-color: var(--primary-color);
  color: #fff;
}

.btn-primary:hover:not(:disabled) {
  background: var(--primary-hover);
  color: #fff;
}

.btn-secondary {
  background: var(--bg-secondary, #fff);
}

.btn-danger {
  border-color: #dc3545;
  color: #dc3545;
}

.btn-danger.confirming {
  background: #dc3545;
  color: #fff;
}

.btn-sm {
  padding: 4px 10px;
  font-size: 12px;
}

/* 编辑表单 */
.edit-label {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary, #666);
  margin-top: 8px;
}

.edit-input,
.edit-textarea {
  width: 100%;
  padding: 8px 12px;
  font-size: 14px;
  border: 1px solid var(--border-color, #e8e8e8);
  border-radius: 8px;
  background: var(--bg-primary, #fff);
  color: var(--text-primary, #333);
  outline: none;
  font-family: inherit;
  box-sizing: border-box;
}

.edit-textarea {
  resize: vertical;
  font-family: inherit;
}

.edit-input:focus,
.edit-textarea:focus {
  border-color: var(--primary-color);
}

.tag-editor {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.tag-list {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}

.tag-input-row {
  display: flex;
  gap: 6px;
}

.tag-input-row .edit-input {
  flex: 1;
}

/* 曲目区 */
.tracks-section {
  background: var(--bg-secondary, #fff);
  border-radius: 16px;
  border: 1px solid var(--border-light, #e8e8e8);
  overflow: hidden;
}

.tracks-header {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 16px 20px;
  border-bottom: 1px solid var(--border-light, #e8e8e8);
}

.tracks-title {
  font-size: 16px;
  font-weight: 700;
  margin: 0;
  color: var(--text-primary, #333);
}

.tracks-count {
  font-size: 12px;
  color: var(--text-tertiary, #999);
}

.empty-tracks {
  padding: 40px 20px;
}

.empty-icon {
  font-size: 40px;
  color: var(--text-tertiary, #999);
  opacity: 0.5;
}

/* 可编辑曲目列表 */
.editable-tracks {
  display: flex;
  flex-direction: column;
}

.editable-row {
  display: grid;
  grid-template-columns: 24px 32px 1fr 50px 32px;
  gap: 10px;
  padding: 10px 20px;
  align-items: center;
  border-bottom: 1px solid var(--border-light, #f0f0f0);
  cursor: grab;
  transition: background 0.15s ease;
  font-size: 13px;
}

.editable-row:hover {
  background: var(--bg-hover, rgba(0, 0, 0, 0.02));
}

.editable-row.drag-from {
  opacity: 0.4;
}

.editable-row.drag-over {
  background: var(--primary-light);
  border-top: 2px solid var(--primary-color);
}

.editable-row:last-child {
  border-bottom: none;
}

.row-handle {
  color: var(--text-tertiary, #999);
  cursor: grab;
  display: flex;
  align-items: center;
  justify-content: center;
}

.row-idx {
  color: var(--text-tertiary, #999);
  text-align: center;
  font-variant-numeric: tabular-nums;
}

.row-info {
  min-width: 0;
}

.row-title {
  font-weight: 500;
  color: var(--text-primary, #333);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.row-artist {
  font-size: 11px;
  color: var(--text-secondary, #666);
  margin-top: 2px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.row-dur {
  text-align: right;
  color: var(--text-tertiary, #999);
  font-size: 12px;
  font-variant-numeric: tabular-nums;
}

.row-remove {
  width: 28px;
  height: 28px;
  border: none;
  background: transparent;
  color: var(--text-tertiary, #999);
  cursor: pointer;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 11px;
  transition: all 0.2s ease;
}

.row-remove:hover {
  background: rgba(220, 53, 69, 0.1);
  color: #dc3545;
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
  z-index: 1000;
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
  color: var(--primary-color);
}

.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.2s;
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}

@media (max-width: 767px) {
  .playlist-header {
    grid-template-columns: 1fr;
    gap: 16px;
  }

  .header-cover {
    width: 100%;
    aspect-ratio: 1;
    height: auto;
    max-width: 240px;
    margin: 0 auto;
  }

  .header-actions {
    flex-direction: row;
    flex-wrap: wrap;
  }

  .header-actions .btn {
    flex: 1;
    min-width: 80px;
  }

  .editable-row {
    grid-template-columns: 24px 1fr 32px;
    gap: 8px;
  }

  .row-idx,
  .row-dur {
    display: none;
  }
}
</style>
