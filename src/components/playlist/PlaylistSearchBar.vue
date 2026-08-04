<script setup>
/**
 * PlaylistSearchBar — 歌单搜索输入框 + 每页条数选择
 *
 * 对外只暴露受控的 query / limit 与事件，防抖与搜索逻辑由 usePlaylistSearch 驱动。
 */
defineProps({
  query: { type: String, default: '' },
  limit: { type: Number, default: 30 },
});

const emit = defineEmits(['update:query', 'update:limit', 'input', 'submit', 'clear', 'limit-change']);

function onInput(e) {
  emit('update:query', e.target.value);
  emit('input');
}

function onLimitChange(e) {
  emit('update:limit', Number(e.target.value));
  emit('limit-change');
}
</script>

<template>
  <div class="search-row">
    <div class="search-box">
      <HIcon name="search" class="search-icon" />
      <input
        :value="query"
        type="text"
        placeholder="输入歌单名、关键词..."
        @input="onInput"
        @keyup.enter="emit('submit')"
      />
      <button v-if="query" class="clear-btn" @click="emit('clear')">
        <HIcon name="x-lg" />
      </button>
    </div>
    <select :value="limit" class="limit-select" @change="onLimitChange">
      <option :value="20">20</option>
      <option :value="30">30</option>
      <option :value="50">50</option>
    </select>
  </div>
</template>

<style scoped>
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
  border-color: var(--primary-color);
  box-shadow: 0 0 0 4px var(--primary-light);
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
</style>