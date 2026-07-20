<template>
  <div ref="rootRef" class="lyric-priority-view">
    <div class="section-header">
      <h2 class="section-title">歌词优先级</h2>
      <p class="section-subtitle">
        当一首歌存在多个来源的歌词时，按以下顺序选择"最佳"歌词用于显示
      </p>
    </div>

    <!-- 来源优先级 -->
    <div class="settings-section">
      <h3 class="block-title">
        <i class="bi bi-list-ol"></i>
        来源优先级
      </h3>
      <p class="block-hint">
        拖拽排序或使用上下按钮调整；列表中越靠前的来源优先级越高
      </p>

      <ul class="source-list" v-if="orderedSources.length">
        <li
          v-for="(kind, idx) in orderedSources"
          :key="kind"
          class="source-item"
          :class="{ 'is-dragging': draggingKind === kind }"
          :draggable="true"
          @dragstart="onDragStart($event, kind)"
          @dragover.prevent="onDragOver"
          @drop.prevent="onDrop(kind)"
          @dragend="onDragEnd"
        >
          <div class="source-rank">{{ idx + 1 }}</div>
          <div class="source-info">
            <span class="source-name">{{ displayName(kind) }}</span>
            <span class="source-status" :class="availabilityClass(kind)">
              {{ availabilityLabel(kind) }}
            </span>
          </div>
          <div class="source-actions">
            <button
              class="icon-btn"
              :disabled="idx === 0"
              @click="moveSourceUp(kind)"
              title="上移"
            >
              <i class="bi bi-chevron-up"></i>
            </button>
            <button
              class="icon-btn"
              :disabled="idx === orderedSources.length - 1"
              @click="moveSourceDown(kind)"
              title="下移"
            >
              <i class="bi bi-chevron-down"></i>
            </button>
            <i class="bi bi-grip-vertical drag-handle" title="拖拽排序"></i>
          </div>
        </li>
      </ul>
      <div v-else class="empty-state">
        <i class="bi bi-inbox"></i>
        <span>暂无来源</span>
      </div>
    </div>

    <!-- 格式偏好 -->
    <div class="settings-section">
      <h3 class="block-title">
        <i class="bi bi-type-strikethrough"></i>
        格式偏好
      </h3>

      <div class="setting-item">
        <div class="setting-info">
          <label class="setting-label">优先逐字歌词</label>
          <span class="setting-desc">
            开启后，同一来源中逐字歌词（YRC/QRC/TTML）优先于行级 LRC，LRC 优先于纯文本
          </span>
        </div>
        <div class="setting-control">
          <label class="toggle">
            <input type="checkbox" v-model="state.prefer_word_by_word" />
            <span class="toggle-slider"></span>
          </label>
        </div>
      </div>

      <div class="setting-item">
        <div class="setting-info">
          <label class="setting-label">本地有逐字时仍尝试在线获取</label>
          <span class="setting-desc">
            用于将本地 LRC 升级为在线 YRC/TTML 等更精细的逐字格式；会增加网络请求
          </span>
        </div>
        <div class="setting-control">
          <label class="toggle">
            <input
              type="checkbox"
              v-model="state.fallback_to_online_for_word_by_word"
            />
            <span class="toggle-slider"></span>
          </label>
        </div>
      </div>
    </div>

    <!-- 说明 -->
    <div class="settings-section">
      <h3 class="block-title">
        <i class="bi bi-info-circle"></i>
        评分规则
      </h3>
      <div class="rule-desc">
        <p>最终得分 = <code>source_rank × 100 + format_weight</code></p>
        <ul>
          <li><b>source_rank</b>：来源在上方列表中的位置（首位 = 列表长度，依次递减）</li>
          <li><b>format_weight</b>：仅在"优先逐字歌词"开启时计入
            <ul>
              <li>TTML = 50, YRC = 45, QRC = 40, LRC = 20, 纯文本 = 0</li>
            </ul>
          </li>
          <li>同一来源内：逐字 &gt; LRC &gt; 纯文本</li>
          <li>跨来源：除非格式权重跨越 100 分差，否则排在前面的来源胜出</li>
        </ul>
      </div>
    </div>

    <!-- 重置 -->
    <div class="settings-section">
      <h3 class="block-title">
        <i class="bi bi-arrow-counterclockwise"></i>
        重置
      </h3>
      <div class="setting-item">
        <div class="setting-info">
          <label class="setting-label">恢复默认</label>
          <span class="setting-desc">将来源顺序和格式偏好恢复为出厂默认值</span>
        </div>
        <div class="setting-control">
          <button class="reset-btn" @click="onReset">恢复默认</button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup>
import { computed, ref, onMounted, useTemplateRef } from 'vue';
import { useAnime } from '@/composables/useAnime.js';
import {
  LyricPreferenceStore,
  moveSourceUp,
  moveSourceDown,
  moveSourceTo,
  resetToDefaults,
} from '@/stores/lyricPreference.js';
import { LYRIC_SOURCE_DISPLAY_NAMES } from '@/api/musicSource/lyricPreference.js';

const { state, availableSources } = LyricPreferenceStore;

// 按 state.source_order 排序的来源列表
const orderedSources = computed(() => state.source_order.slice());

// 拖拽状态：当前正在拖拽的来源 kind
const draggingKind = ref(null);

function displayName(kind) {
  return LYRIC_SOURCE_DISPLAY_NAMES[kind] ?? kind;
}

function sourceInfo(kind) {
  return availableSources.find((s) => s.source_name === kind);
}

function availabilityLabel(kind) {
  const info = sourceInfo(kind);
  if (!info) return '未知';
  return info.available ? '可用' : '未启用';
}

function availabilityClass(kind) {
  const info = sourceInfo(kind);
  if (!info) return 'status-unknown';
  return info.available ? 'status-available' : 'status-disabled';
}

// ── 拖拽排序 ───────────────────────────────────────────
// 使用原生 HTML5 drag&drop API，无需引入额外依赖
function onDragStart(evt, kind) {
  draggingKind.value = kind;
  evt.dataTransfer.effectAllowed = 'move';
  // Firefox 需要 setData 才能触发拖拽
  try { evt.dataTransfer.setData('text/plain', kind); } catch (_) {}
}

function onDragOver(evt) {
  // 必须 preventDefault 才能触发 drop
  evt.dataTransfer.dropEffect = 'move';
}

function onDrop(targetKind) {
  const sourceKind = draggingKind.value;
  if (sourceKind && sourceKind !== targetKind) {
    const toIndex = state.source_order.indexOf(targetKind);
    if (toIndex >= 0) moveSourceTo(sourceKind, toIndex);
  }
  draggingKind.value = null;
}

function onDragEnd() {
  draggingKind.value = null;
}

// ── 重置 ─────────────────────────────────────────────────
function onReset() {
  if (!confirm('确定要将歌词优先级恢复为默认值吗？')) return;
  resetToDefaults();
}

// ── 入场动画 ───────────────────────────────────────────
const rootRef = useTemplateRef('root');
const { run } = useAnime(() => rootRef.value);

onMounted(async () => {
  // 进入设置页时刷新一次可用来源（用户可能刚刚在音乐源管理页启用/禁用了某个来源）
  await LyricPreferenceStore.refreshAvailableSources();
  run(({ animate, stagger, presets }) => {
    animate('.section-header', { ...presets.fadeIn });
    animate('.settings-section', { ...presets.fadeInUp, delay: stagger(80) });
  });
});
</script>

<style scoped>
.lyric-priority-view {
  max-width: 720px;
  margin: 0 auto;
}

.section-header {
  margin-bottom: 24px;
}

.section-title {
  font-size: 24px;
  font-weight: 800;
  margin: 0 0 6px;
  color: var(--text-primary, #333);
}

.section-subtitle {
  font-size: 13px;
  color: var(--text-secondary, #666);
  margin: 0;
  line-height: 1.5;
}

.settings-section {
  background: var(--bg-secondary, #fff);
  border-radius: 16px;
  padding: 20px 24px;
  margin-bottom: 16px;
  border: 1px solid var(--border-light, #e8e8e8);
}

.block-title {
  font-size: 15px;
  font-weight: 700;
  margin: 0 0 8px;
  color: var(--text-primary, #333);
  display: flex;
  align-items: center;
  gap: 8px;
}

.block-title i {
  color: var(--primary-color, #0078d7);
}

.block-hint {
  font-size: 12px;
  color: var(--text-secondary, #666);
  margin: 0 0 16px;
  line-height: 1.5;
}

/* 来源列表 */
.source-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.source-item {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 12px 14px;
  background: var(--bg-primary, #fafafa);
  border: 1px solid var(--border-light, #e8e8e8);
  border-radius: 12px;
  cursor: grab;
  transition: background 0.15s, border-color 0.15s, transform 0.15s, opacity 0.15s;
  user-select: none;
}

.source-item:hover {
  border-color: var(--primary-color, #0078d7);
  background: var(--bg-hover, rgba(0, 120, 215, 0.04));
}

.source-item:active {
  cursor: grabbing;
}

.source-item.is-dragging {
  opacity: 0.4;
  transform: scale(0.98);
}

.source-rank {
  width: 28px;
  height: 28px;
  border-radius: 50%;
  background: var(--primary-light, rgba(0, 120, 215, 0.1));
  color: var(--primary-color, #0078d7);
  font-size: 13px;
  font-weight: 700;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.source-info {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}

.source-name {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary, #333);
}

.source-status {
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 8px;
  font-weight: 500;
}

.status-available {
  background: rgba(34, 197, 94, 0.12);
  color: #16a34a;
}

.status-disabled {
  background: rgba(234, 88, 12, 0.12);
  color: #c2410c;
}

.status-unknown {
  background: rgba(107, 114, 128, 0.12);
  color: #6b7280;
}

.source-actions {
  display: flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
}

.icon-btn {
  width: 28px;
  height: 28px;
  border: none;
  background: transparent;
  color: var(--text-secondary, #666);
  cursor: pointer;
  border-radius: 6px;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: background 0.15s, color 0.15s;
}

.icon-btn:hover:not(:disabled) {
  background: var(--bg-hover, rgba(0, 0, 0, 0.06));
  color: var(--text-primary, #333);
}

.icon-btn:disabled {
  opacity: 0.3;
  cursor: not-allowed;
}

.drag-handle {
  color: var(--text-tertiary, #999);
  cursor: grab;
  margin-left: 4px;
  font-size: 16px;
}

.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 32px;
  color: var(--text-tertiary, #999);
  font-size: 13px;
}

.empty-state i {
  font-size: 28px;
}

/* 设置项 */
.setting-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 0;
  border-bottom: 1px solid var(--border-light, #e8e8e8);
  gap: 16px;
}

.setting-item:last-child {
  border-bottom: none;
}

.setting-info {
  flex: 1;
  min-width: 0;
  padding-right: 16px;
}

.setting-label {
  display: block;
  font-size: 14px;
  font-weight: 500;
  color: var(--text-primary, #333);
  margin-bottom: 3px;
}

.setting-desc {
  display: block;
  font-size: 12px;
  color: var(--text-secondary, #666);
  line-height: 1.5;
}

.setting-control {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-shrink: 0;
}

/* 开关 */
.toggle {
  position: relative;
  display: inline-block;
  width: 44px;
  height: 22px;
}

.toggle input {
  opacity: 0;
  width: 0;
  height: 0;
}

.toggle-slider {
  position: absolute;
  cursor: pointer;
  inset: 0;
  background-color: var(--border-color, #ccc);
  transition: 0.3s;
  border-radius: 22px;
}

.toggle-slider:before {
  position: absolute;
  content: "";
  height: 16px;
  width: 16px;
  left: 3px;
  bottom: 3px;
  background-color: white;
  transition: 0.3s;
  border-radius: 50%;
}

.toggle input:checked + .toggle-slider {
  background-color: var(--primary-color, #0078d7);
}

.toggle input:checked + .toggle-slider:before {
  transform: translateX(22px);
}

/* 评分规则说明 */
.rule-desc {
  font-size: 13px;
  color: var(--text-secondary, #555);
  line-height: 1.6;
}

.rule-desc p {
  margin: 0 0 10px;
}

.rule-desc ul {
  margin: 0 0 0 20px;
  padding: 0;
}

.rule-desc li {
  margin-bottom: 4px;
}

.rule-desc code {
  background: var(--bg-code, rgba(0, 0, 0, 0.06));
  padding: 1px 6px;
  border-radius: 4px;
  font-family: 'Consolas', 'Monaco', monospace;
  font-size: 12px;
}

.rule-desc b {
  color: var(--text-primary, #333);
  font-weight: 600;
}

/* 重置按钮 */
.reset-btn {
  padding: 7px 14px;
  font-size: 13px;
  border: 1px solid var(--border-color, #e8e8e8);
  border-radius: 8px;
  background: var(--bg-primary, #fff);
  color: var(--text-primary, #333);
  cursor: pointer;
  transition: all 0.2s;
}

.reset-btn:hover {
  border-color: #e34a4a;
  color: #e34a4a;
}

@media (max-width: 767px) {
  .setting-item {
    flex-direction: column;
    align-items: flex-start;
    gap: 10px;
  }

  .setting-control {
    width: 100%;
    justify-content: space-between;
  }

  .source-item {
    padding: 10px 12px;
    gap: 10px;
  }

  .source-name {
    font-size: 13px;
  }
}
</style>
