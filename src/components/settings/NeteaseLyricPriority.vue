<template>
  <div class="settings-section">
    <h3 class="block-title">
      <HIcon name="music-note-list" />
      歌词优先级
    </h3>

    <div class="setting-item column">
      <div class="setting-info">
        <label class="setting-label">来源优先级</label>
        <span class="setting-desc">从前到后依次尝试；勾选的来源才会被使用</span>
      </div>
      <div class="source-order">
        <div
          v-for="(src, idx) in sourceOrderItems"
          :key="src.value"
          class="source-item"
        >
          <label class="source-check">
            <input
              type="checkbox"
              v-model="src.enabled"
              @change="onSourceOrderChange"
            />
            <span>{{ src.label }}</span>
          </label>
          <div class="source-rank">
            <button
              class="rank-btn"
              @click="moveSource(idx, -1)"
              :disabled="idx === 0"
              title="上移"
            ><HIcon name="chevron-up" /></button>
            <button
              class="rank-btn"
              @click="moveSource(idx, 1)"
              :disabled="idx === sourceOrderItems.length - 1"
              title="下移"
            ><HIcon name="chevron-down" /></button>
            <span class="rank-num">#{{ idx + 1 }}</span>
          </div>
        </div>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">优先逐字歌词</label>
        <span class="setting-desc">同一来源中，逐字（YRC/QRC/TTML）优先于行级 LRC 优先于纯文本</span>
      </div>
      <div class="setting-control">
        <label class="toggle">
          <input type="checkbox" v-model="preferWordByWord" />
          <span class="toggle-slider"></span>
        </label>
      </div>
    </div>

    <div class="setting-item">
      <div class="setting-info">
        <label class="setting-label">本地有逐字时仍尝试在线获取</label>
        <span class="setting-desc">用于本地 LRC 升级到 YRC；本地已是逐字则不会重复获取</span>
      </div>
      <div class="setting-control">
        <label class="toggle">
          <input
            type="checkbox"
            v-model="fallbackToOnlineForWordByWord"
          />
          <span class="toggle-slider"></span>
        </label>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, computed, watch } from 'vue';

const props = defineProps({
  pref: { type: Object, required: true },
});

const emit = defineEmits(['save']);

// 来源选项（顺序可调整）
const sourceOptions = [
  { value: 'local', label: '本地（音频嵌入 / .lrc 文件）' },
  { value: 'netease', label: '网易云（YRC / LRC）' },
];

// 当前展示的来源顺序（含启用状态）
const sourceOrderItems = ref([]);

function rebuildSourceOrderItems() {
  // 按 pref.source_order 顺序展示，未在其中的来源附加在末尾并标记为禁用
  const ordered = [];
  for (const value of props.pref.source_order) {
    const opt = sourceOptions.find((o) => o.value === value);
    if (opt) ordered.push({ ...opt, enabled: true });
  }
  for (const opt of sourceOptions) {
    if (!ordered.find((o) => o.value === opt.value)) {
      ordered.push({ ...opt, enabled: false });
    }
  }
  sourceOrderItems.value = ordered;
}

function onSourceOrderChange() {
  // 仅保留勾选的来源
  props.pref.source_order = sourceOrderItems.value
    .filter((s) => s.enabled)
    .map((s) => s.value);
  emit('save');
}

function moveSource(idx, delta) {
  const arr = sourceOrderItems.value;
  const newIdx = idx + delta;
  if (newIdx < 0 || newIdx >= arr.length) return;
  [arr[idx], arr[newIdx]] = [arr[newIdx], arr[idx]];
  onSourceOrderChange();
}

// 两个偏好开关：直接写入 pref 并触发保存
const preferWordByWord = computed({
  get: () => props.pref.prefer_word_by_word,
  set: (v) => {
    props.pref.prefer_word_by_word = v;
    emit('save');
  },
});

const fallbackToOnlineForWordByWord = computed({
  get: () => props.pref.fallback_to_online_for_word_by_word,
  set: (v) => {
    props.pref.fallback_to_online_for_word_by_word = v;
    emit('save');
  },
});

// 外部重新加载 pref 后重建顺序列表
watch(
  () => props.pref.source_order,
  () => rebuildSourceOrderItems(),
  { immediate: true }
);
</script>

<style scoped>
.source-order {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.source-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 14px;
  background: var(--bg-hover, rgba(0, 0, 0, 0.02));
  border-radius: 8px;
  border: 1px solid var(--border-light, #e8e8e8);
}

.source-check {
  display: flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
  font-size: 13px;
  color: var(--text-primary, #333);
}

.source-check input {
  margin: 0;
}

.source-rank {
  display: flex;
  align-items: center;
  gap: 6px;
}

.rank-btn {
  width: 24px;
  height: 24px;
  padding: 0;
  border: 1px solid var(--border-color, #e8e8e8);
  background: var(--bg-primary, #fff);
  color: var(--text-secondary, #666);
  border-radius: 4px;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 11px;
}

.rank-btn:hover:not(:disabled) {
  border-color: var(--primary-color);
  color: var(--primary-color);
}

.rank-btn:disabled {
  opacity: 0.3;
  cursor: not-allowed;
}

.rank-num {
  font-size: 11px;
  color: var(--text-tertiary, #999);
  min-width: 24px;
  text-align: right;
}
</style>