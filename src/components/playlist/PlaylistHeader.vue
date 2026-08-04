<script setup>
/**
 * PlaylistHeader — 歌单详情页头部
 *
 * 职责：
 *  - 展示封面 / 名称 / 描述 / 标签 / 统计信息
 *  - 编辑模式：名称 / 描述 / 标签编辑表单
 *  - 操作按钮：播放全部 / 批量下载 / 编辑 / 删除（二次确认）
 */
const props = defineProps({
  playlist: { type: Object, required: true },
  songs: { type: Array, default: () => [] },
  isEditing: { type: Boolean, default: false },
  editForm: { type: Object, required: true },
  tagInput: { type: String, default: '' },
  saving: { type: Boolean, default: false },
  pendingDelete: { type: Boolean, default: false },
  sourceLabel: { type: String, default: '' },
});

const emit = defineEmits([
  'update:tagInput',
  'play-all',
  'batch',
  'start-edit',
  'confirm-delete',
  'cancel-edit',
  'save',
  'add-tag',
  'remove-tag',
]);

function formatTime(ts) {
  if (!ts) return '';
  const d = new Date(ts);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}
</script>

<template>
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
              <button class="tag-remove" @click="emit('remove-tag', t)">×</button>
            </span>
          </div>
          <div class="tag-input-row">
            <input
              :value="tagInput"
              @input="emit('update:tagInput', $event.target.value)"
              type="text"
              class="edit-input"
              placeholder="添加标签后回车"
              @keydown.enter.prevent="emit('add-tag')"
            />
            <button class="btn btn-secondary btn-sm" @click="emit('add-tag')">添加</button>
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
          @click="emit('play-all')"
        >
          <HIcon name="play-fill" /> 播放全部
        </button>
        <button
          class="btn btn-secondary"
          :disabled="songs.length === 0"
          @click="emit('batch')"
          title="将网易云曲目批量下载到本地（后台静默执行）"
        >
          <HIcon name="cloud-arrow-down" /> 批量下载
        </button>
        <button class="btn btn-secondary" @click="emit('start-edit')">
          <HIcon name="pencil" /> 编辑
        </button>
        <button
          class="btn btn-danger"
          :class="{ confirming: pendingDelete }"
          @click="emit('confirm-delete')"
        >
          {{ pendingDelete ? '确认删除' : '删除' }}
        </button>
      </template>
      <template v-else>
        <button class="btn btn-secondary" @click="emit('cancel-edit')" :disabled="saving">取消</button>
        <button class="btn btn-primary" @click="emit('save')" :disabled="saving">
          {{ saving ? '保存中…' : '保存' }}
        </button>
      </template>
    </div>
  </header>
</template>

<style scoped>
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
}
</style>