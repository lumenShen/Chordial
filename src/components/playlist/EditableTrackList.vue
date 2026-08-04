<script setup>
/**
 * EditableTrackList — 歌单编辑模式下的可拖拽曲目列表
 *
 * 职责：
 *  - 渲染带有拖拽重排与移除功能的曲目行
 *  - 拖拽状态由父级（composable）维护，通过事件上报
 */
import { formatDuration } from '@/utils/format.js';

const props = defineProps({
  songs: { type: Array, default: () => [] },
  dragIndex: { type: Number, default: null },
  dragOverIndex: { type: Number, default: null },
});

const emit = defineEmits([
  'drag-start',
  'drag-over',
  'drag-leave',
  'drop',
  'drag-end',
  'remove',
]);
</script>

<template>
  <div class="editable-tracks">
    <div
      v-for="(song, idx) in songs"
      :key="song.id"
      class="editable-row"
      :class="{
        'drag-from': dragIndex === idx,
        'drag-over': dragOverIndex === idx && dragIndex !== idx,
      }"
      draggable="true"
      @dragstart="emit('drag-start', idx)"
      @dragover="emit('drag-over', idx, $event)"
      @dragleave="emit('drag-leave')"
      @drop="emit('drop', idx)"
      @dragend="emit('drag-end')"
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
      <button class="row-remove" @click="emit('remove', song.id)" title="从歌单移除">
        <HIcon name="x-lg" />
      </button>
    </div>
  </div>
</template>

<style scoped>
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

@media (max-width: 767px) {
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