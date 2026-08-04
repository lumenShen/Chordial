<script setup>
/**
 * EndlessPlaySection - 侧边栏「无尽播放」区段
 *
 * 负责智能混音 / 完全随机 / 网易云电台（私人 FM、私人 DJ、跑步漫游、相似音乐）
 * 的展示与交互。所有动作经事件上抛，由父组件统一编排。
 */
import { computed } from 'vue';

const props = defineProps({
  open: { type: Boolean, default: true },
  busyItem: { type: String, default: '' },
  neteaseEnabled: { type: Boolean, default: false },
  endlessMode: { type: String, default: null },
  sportBpmOpen: { type: Boolean, default: false },
  sportBpm: { type: Number, default: 150 },
  currentNeteaseSongId: { type: String, default: null },
});

const emit = defineEmits([
  'toggle',
  'toggle-bpm',
  'update:bpm',
  'start-smart-mix',
  'start-shuffle',
  'start-personal-fm',
  'start-private-dj',
  'start-sport-run',
  'start-similar',
]);

// bpm 输入双向绑定（经事件上抛）
const bpmValue = computed({
  get: () => props.sportBpm,
  set: (v) => emit('update:bpm', v),
});
</script>

<template>
  <section class="sidebar-section">
    <button class="section-title-btn" @click="emit('toggle')">
      <HIcon name="arrow-repeat" :size="15" class="title-icon" />
      <span>无尽播放</span>
      <HIcon :name="open ? 'chevron-down' : 'chevron-right'" :size="14" class="title-chevron" />
    </button>

    <div v-show="open" class="section-body">
      <!-- 智能混音 -->
      <button class="endless-item" :disabled="busyItem === 'smart-mix'" @click="emit('start-smart-mix')">
        <span class="item-icon hot">
          <HIcon name="crown" :size="16" />
        </span>
        <span class="item-text">
          <span class="item-name">智能混音</span>
          <span class="item-desc">常听高频 + 其余曲库交错循环</span>
        </span>
        <span v-if="endlessMode === 'smart-mix'" class="live-dot">播放中</span>
      </button>

      <!-- 完全随机 -->
      <button class="endless-item" :disabled="busyItem === 'shuffle'" @click="emit('start-shuffle')">
        <span class="item-icon">
          <HIcon name="shuffle" :size="16" />
        </span>
        <span class="item-text">
          <span class="item-name">完全随机</span>
          <span class="item-desc">从音乐库全库随机播放</span>
        </span>
        <span v-if="endlessMode === 'shuffle'" class="live-dot">播放中</span>
      </button>

      <!-- 网易云电台（启用网易云来源时显示） -->
      <template v-if="neteaseEnabled">
        <div class="sub-divider"></div>
        <div class="sub-caption">
          <HIcon name="broadcast" :size="13" />
          <span>网易云电台</span>
        </div>

        <button class="endless-item" :disabled="busyItem === 'personal-fm'" @click="emit('start-personal-fm')">
          <span class="item-icon netease">
            <HIcon name="mic" :size="16" />
          </span>
          <span class="item-text">
            <span class="item-name">私人 FM</span>
            <span class="item-desc">智能推荐符合口味的歌曲</span>
          </span>
        </button>

        <button class="endless-item" :disabled="busyItem === 'private-dj'" @click="emit('start-private-dj')">
          <span class="item-icon netease">
            <HIcon name="music-note" :size="16" />
          </span>
          <span class="item-text">
            <span class="item-name">私人 DJ</span>
            <span class="item-desc">AI 声音推荐 + 歌曲</span>
          </span>
        </button>

        <div class="endless-item bpm-item">
          <button class="bpm-trigger" :disabled="busyItem === 'sport-run'" @click="emit('toggle-bpm')">
            <span class="item-icon netease">
              <HIcon name="geo-alt" :size="16" />
            </span>
            <span class="item-text">
              <span class="item-name">跑步漫游</span>
              <span class="item-desc">按步频匹配歌曲</span>
            </span>
          </button>
          <div v-if="sportBpmOpen" class="bpm-control" @click.stop>
            <input
              v-model.number="bpmValue"
              type="number"
              min="60"
              max="220"
              step="5"
              placeholder="步频 BPM"
            />
            <button class="bpm-go" @click="emit('start-sport-run')">开始</button>
          </div>
        </div>

        <button
          class="endless-item"
          :disabled="busyItem === 'similar' || !currentNeteaseSongId"
          :title="currentNeteaseSongId ? '根据当前歌曲推荐相似歌曲' : '当前歌曲没有网易云来源'"
          @click="emit('start-similar')"
        >
          <span class="item-icon netease">
            <HIcon name="broadcast" :size="16" />
          </span>
          <span class="item-text">
            <span class="item-name">相似音乐</span>
            <span class="item-desc">基于当前歌曲推荐</span>
          </span>
        </button>
      </template>
    </div>
  </section>
</template>

<style scoped>
/* ── 区段 ─────────────────────────────────────────────── */
.sidebar-section {
  margin-bottom: 4px;
}

.section-title-btn {
  display: flex;
  align-items: center;
  gap: 7px;
  flex: 1;
  min-width: 0;
  padding: 6px 8px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--font-size-sm);
  font-weight: 600;
  letter-spacing: 0.03em;
  cursor: pointer;
  border-radius: var(--radius-sm);
  text-transform: uppercase;
  transition: color var(--transition-fast), background var(--transition-fast);
}

.section-title-btn:hover {
  color: var(--text-primary);
  background: var(--bg-hover);
}

.title-icon {
  color: var(--text-tertiary);
  flex-shrink: 0;
}

.title-chevron {
  color: var(--text-tertiary);
  margin-left: auto;
  flex-shrink: 0;
}

.section-body {
  padding: 2px 0 6px;
}

/* ── 无尽播放条目 ─────────────────────────────────────── */
.endless-item {
  display: flex;
  align-items: center;
  gap: 9px;
  width: 100%;
  padding: 7px 8px;
  border: none;
  border-radius: var(--radius-md);
  background: transparent;
  color: var(--text-primary);
  cursor: pointer;
  text-align: left;
  transition: background var(--transition-fast);
}

.endless-item:hover {
  background: var(--bg-hover);
}

.endless-item:disabled {
  opacity: 0.55;
  cursor: default;
}

.item-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border-radius: var(--radius-sm);
  background: var(--bg-tertiary);
  color: var(--text-secondary);
  flex-shrink: 0;
}

.item-icon.hot {
  background: var(--primary-light);
  color: var(--primary-color);
}

.item-icon.netease {
  background: rgba(197, 17, 17, 0.10);
  color: #c51111;
}

.item-text {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}

.item-name {
  font-size: var(--font-size-base);
  font-weight: 500;
  line-height: 1.3;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.item-desc {
  font-size: var(--font-size-xs);
  color: var(--text-tertiary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.live-dot {
  flex-shrink: 0;
  font-size: var(--font-size-xs);
  font-weight: 600;
  color: var(--primary-color);
  padding: 2px 6px;
  border-radius: 10px;
  background: var(--primary-light);
}

.sub-divider {
  height: 1px;
  background: var(--border-light);
  margin: 8px 8px 6px;
}

.sub-caption {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 8px 6px;
  font-size: var(--font-size-xs);
  font-weight: 600;
  color: var(--text-tertiary);
  letter-spacing: 0.04em;
  text-transform: uppercase;
}

/* 跑步漫游 bpm */
.bpm-item {
  display: block;
  padding: 0;
}

.bpm-trigger {
  display: flex;
  align-items: center;
  gap: 9px;
  width: 100%;
  padding: 7px 8px;
  border: none;
  background: transparent;
  color: var(--text-primary);
  cursor: pointer;
  text-align: left;
  border-radius: var(--radius-md);
}

.bpm-trigger:hover {
  background: var(--bg-hover);
}

.bpm-control {
  display: flex;
  gap: 6px;
  padding: 4px 8px 8px 43px;
}

.bpm-control input {
  width: 72px;
  padding: 5px 8px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--bg-primary);
  color: var(--text-primary);
  font-size: var(--font-size-sm);
  outline: none;
}

.bpm-control input:focus {
  border-color: var(--primary-color);
}

.bpm-go {
  padding: 5px 12px;
  border: none;
  border-radius: var(--radius-sm);
  background: var(--primary-color);
  color: var(--primary-contrast);
  font-size: var(--font-size-sm);
  font-weight: 500;
  cursor: pointer;
}
</style>