<script setup>
// 双绑模型：query / entityType / sourceName / limitPerType 由父层（useSearch）持有
const query = defineModel('query', { type: String, default: '' });
const entityType = defineModel('entityType', { type: String, default: 'all' });
const sourceName = defineModel('sourceName', { type: String, default: 'all' });
const limitPerType = defineModel('limitPerType', { type: Number, default: 50 });

// 只读展示型 props
defineProps({
  searchTimeMs: { type: Number, default: null },
  isLoading: { type: Boolean, default: false },
  totalCount: { type: Number, default: 0 },
});

// 交互事件：防抖输入 / 回车提交 / 清空
const emit = defineEmits(['input', 'submit', 'clear']);

const typeOptions = [
  { value: 'all', label: '全部' },
  { value: 'song', label: '歌曲' },
  { value: 'artist', label: '歌手' },
  { value: 'album', label: '专辑' },
];

const sourceOptions = [
  { value: 'all', label: '全部来源' },
  { value: 'local', label: '本地' },
  { value: 'netease', label: '网易云音乐' },
];
</script>

<template>
  <div class="search-header">
    <h1 class="page-title">搜索</h1>

    <!-- 搜索框 -->
    <div class="search-input-row">
      <div class="search-box">
        <HIcon name="search" class="search-icon" />
        <input
          v-model="query"
          type="text"
          placeholder="搜索歌曲、歌手、专辑..."
          @input="emit('input')"
          @keyup.enter="emit('submit')"
        />
        <button v-if="query" class="clear-btn" title="清空" @click="emit('clear')">
          <HIcon name="x-lg" />
        </button>
      </div>
    </div>

    <!-- 筛选器 -->
    <div class="filter-row">
      <div class="filter-group">
        <span class="filter-label">类型</span>
        <div class="tab-group">
          <button
            v-for="opt in typeOptions"
            :key="opt.value"
            class="tab-btn"
            :class="{ active: entityType === opt.value }"
            @click="entityType = opt.value"
          >
            {{ opt.label }}
          </button>
        </div>
      </div>

      <div class="filter-group">
        <span class="filter-label">来源</span>
        <select v-model="sourceName" class="source-select">
          <option v-for="opt in sourceOptions" :key="opt.value" :value="opt.value">
            {{ opt.label }}
          </option>
        </select>
      </div>

      <div class="filter-group">
        <span class="filter-label">每类上限</span>
        <input
          v-model.number="limitPerType"
          type="number"
          min="1"
          max="500"
          class="limit-input"
        />
      </div>

      <div v-if="searchTimeMs !== null && !isLoading" class="search-meta">
        {{ totalCount }} 条结果 · {{ searchTimeMs }}ms
      </div>
    </div>
  </div>
</template>

<style scoped>
.search-header {
  margin-bottom: 24px;
}

/* page-title 使用全局统一样式（28px/800），不再本地覆盖 */

/* ── 搜索框 ── */
.search-input-row {
  margin-bottom: 16px;
}

.search-box {
  position: relative;
  display: flex;
  align-items: center;
  max-width: 640px;
}

.search-icon {
  position: absolute;
  left: 14px;
  font-size: 16px;
  color: var(--text-tertiary);
  pointer-events: none;
}

.search-box input {
  width: 100%;
  height: 44px;
  padding: 0 44px 0 44px;
  border: 1px solid var(--border-light);
  border-radius: 22px;
  background: var(--bg-glass);
  color: var(--text-primary);
  font-size: 15px;
  font-weight: 400;
  transition: all var(--transition-normal);
  backdrop-filter: saturate(180%) blur(12px);
  -webkit-backdrop-filter: saturate(180%) blur(12px);
}

.search-box input:focus {
  outline: none;
  border-color: var(--primary-color);
  background: var(--bg-secondary);
  box-shadow: 0 0 0 4px var(--primary-light);
}

.search-box input::placeholder {
  color: var(--text-tertiary);
}

.clear-btn {
  position: absolute;
  right: 10px;
  width: 28px;
  height: 28px;
  border: none;
  border-radius: 50%;
  background: var(--bg-hover);
  color: var(--text-secondary);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 11px;
  transition: all var(--transition-fast);
}

.clear-btn:hover {
  background: var(--bg-active);
  color: var(--text-primary);
}

/* ── 筛选器 ── */
.filter-row {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 16px 20px;
}

.filter-group {
  display: flex;
  align-items: center;
  gap: 8px;
}

.filter-label {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-secondary);
}

.tab-group {
  display: flex;
  background: var(--bg-glass);
  border: 1px solid var(--border-light);
  border-radius: 10px;
  padding: 2px;
  backdrop-filter: saturate(180%) blur(12px);
  -webkit-backdrop-filter: saturate(180%) blur(12px);
}

.tab-btn {
  padding: 6px 14px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  font-size: 13px;
  font-weight: 500;
  border-radius: 8px;
  cursor: pointer;
  transition: all var(--transition-fast);
}

.tab-btn:hover {
  color: var(--text-primary);
}

.tab-btn.active {
  background: var(--primary-color);
  color: #fff;
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.15);
}

.source-select,
.limit-input {
  height: 32px;
  padding: 0 10px;
  border: 1px solid var(--border-light);
  border-radius: 8px;
  background: var(--bg-glass);
  color: var(--text-primary);
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: all var(--transition-fast);
}

.source-select:focus,
.limit-input:focus {
  outline: none;
  border-color: var(--primary-color);
  box-shadow: 0 0 0 3px var(--primary-light);
}

.limit-input {
  width: 80px;
  cursor: text;
}

.search-meta {
  margin-left: auto;
  font-size: 12px;
  color: var(--text-tertiary);
  font-variant-numeric: tabular-nums;
}

/* ── 响应式 ── */
@media (max-width: 767px) {
  .filter-row {
    gap: 12px;
  }

  .search-meta {
    margin-left: 0;
    width: 100%;
    text-align: right;
  }
}

@media (max-width: 480px) {
  .filter-group {
    width: 100%;
    justify-content: space-between;
  }

  .tab-group {
    flex: 1;
  }

  .tab-btn {
    flex: 1;
    padding: 6px 8px;
    font-size: 12px;
  }
}

@media (prefers-color-scheme: dark) {
  .search-box input {
    background: var(--bg-glass);
    border-color: var(--border-light);
  }
}
</style>