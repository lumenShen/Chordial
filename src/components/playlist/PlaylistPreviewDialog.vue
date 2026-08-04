<script setup>
/**
 * PlaylistPreviewDialog — 歌单详情预览弹窗
 *
 * 含 fade 过渡，展示封面 / 描述 / 标签 / 曲目列表，并提供保存到本地操作。
 */
defineProps({
  detail: { type: Object, default: null },
  tracks: { type: Array, default: () => [] },
  loading: { type: Boolean, default: false },
  saving: { type: Boolean, default: false },
});

const emit = defineEmits(['close', 'save']);

function formatDuration(dt) {
  return `${Math.floor((dt || 0) / 60000)}:${String(Math.floor(((dt || 0) % 60000) / 1000)).padStart(2, '0')}`;
}
</script>

<template>
  <transition name="fade">
    <div v-if="detail" class="dialog-mask" @click.self="emit('close')">
      <div class="preview-dialog">
        <div class="preview-header">
          <div class="preview-cover">
            <img
              v-if="detail.coverImgUrl"
              :src="detail.coverImgUrl"
              :alt="detail.name"
              @error="(e) => e.target.style.display = 'none'"
            />
            <div v-else class="cover-placeholder">
              <HIcon name="music-note-list" />
            </div>
          </div>
          <div class="preview-meta">
            <h2 class="preview-title">{{ detail.name }}</h2>
            <p class="preview-sub" v-if="detail.creator">
              创建者：{{ detail.creator.nickname }}
            </p>
            <p class="preview-sub">
              曲目数：{{ tracks.length }}
              <span v-if="detail.trackCount"> / {{ detail.trackCount }}</span>
            </p>
            <div class="preview-tags" v-if="detail.tags && detail.tags.length">
              <span v-for="t in detail.tags" :key="t" class="tag">{{ t }}</span>
            </div>
          </div>
          <button class="close-btn" @click="emit('close')">
            <HIcon name="x-lg" />
          </button>
        </div>

        <p v-if="detail.description" class="preview-desc">
          {{ detail.description }}
        </p>

        <div class="preview-actions">
          <button class="btn btn-secondary" @click="emit('close')">取消</button>
          <button
            class="btn btn-primary"
            :disabled="saving || loading"
            @click="emit('save')"
          >
            <HIcon name="cloud-download" />
            {{ saving ? '保存中…' : '保存到本地音乐库' }}
          </button>
        </div>

        <div class="preview-tracks">
          <div v-if="loading" class="tracks-loading">
            <div class="spinner-small"></div> 加载曲目中…
          </div>
          <div v-else-if="tracks.length === 0" class="tracks-empty">
            暂无曲目
          </div>
          <div v-else class="track-list-mini">
            <div
              v-for="(track, idx) in tracks"
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
              <span class="track-dur">{{ formatDuration(track.dt) }}</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  </transition>
</template>

<style scoped>
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

.cover-placeholder {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 36px;
  color: var(--text-tertiary, #999);
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
  background: var(--primary-light);
  color: var(--primary-color);
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

.spinner-small {
  display: inline-block;
  width: 14px;
  height: 14px;
  border: 2px solid var(--border-light, #e8e8e8);
  border-top-color: var(--primary-color);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
  vertical-align: middle;
  margin-right: 6px;
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

@media (max-width: 480px) {
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