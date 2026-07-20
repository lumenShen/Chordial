<script setup>
/**
 * BatchDownloadDialog - 批量下载配置对话框
 *
 * 在发起批量下载前让用户选择：
 * 1. 目标音质等级（standard/higher/exhigh/lossless/hires）
 *    - 当目标音质不可用（VIP/无版权）时，后端会自动向下降级
 * 2. 是否启用去重 + 去重策略
 *    - skip_if_equal: 本地已有相同或更高音质 → 跳过
 *    - replace_if_better: 本地音质 < 目标 → 下载并删除旧文件；本地音质 ≥ 目标 → 跳过
 *
 * 用于 PlaylistDetailView / AlbumDetail 的"批量下载"按钮。
 */
import { ref, watch, computed } from 'vue';

const props = defineProps({
  /** v-model:visible */
  modelValue: {
    type: Boolean,
    default: false,
  },
  /** 待下载曲目数（用于显示提示文字） */
  total: {
    type: Number,
    default: 0,
  },
  /** 启动中（禁用按钮） */
  loading: {
    type: Boolean,
    default: false,
  },
});

const emit = defineEmits(['update:modelValue', 'confirm']);

// ── 选项 ────────────────────────────────────────────────
const LEVELS = [
  { value: 'standard', label: '标准 128kbps MP3' },
  { value: 'higher', label: '较高 192kbps MP3' },
  { value: 'exhigh', label: '极高 320kbps MP3' },
  { value: 'lossless', label: '无损 FLAC 16/44kHz' },
  { value: 'hires', label: 'Hi-Res FLAC 24/96kHz' },
];

const targetLevel = ref('exhigh');
const dedupEnabled = ref(false);
const dedupStrategy = ref('replace_if_better');

// 对话框打开时重置为默认值（避免上次选择残留）
watch(
  () => props.modelValue,
  (visible) => {
    if (visible) {
      targetLevel.value = 'exhigh';
      dedupEnabled.value = false;
      dedupStrategy.value = 'replace_if_better';
    }
  },
);

const canConfirm = computed(() => !props.loading && props.total > 0);

function close() {
  if (props.loading) return;
  emit('update:modelValue', false);
}

function confirm() {
  if (!canConfirm.value) return;
  emit('confirm', {
    targetLevel: targetLevel.value,
    dedupStrategy: dedupEnabled.value ? dedupStrategy.value : null,
  });
}
</script>

<template>
  <transition name="bdd-fade">
    <div v-if="modelValue" class="bdd-overlay" @click.self="close">
      <div class="bdd-dialog" role="dialog" aria-modal="true">
        <header class="bdd-header">
          <i class="bi bi-cloud-arrow-down"></i>
          <h2 class="bdd-title">批量下载配置</h2>
          <button class="bdd-close" :disabled="loading" @click="close" title="取消">
            <i class="bi bi-x-lg"></i>
          </button>
        </header>

        <div class="bdd-body">
          <p class="bdd-tip">
            即将下载 <strong>{{ total }}</strong> 首曲目到本地，请选择目标音质与去重策略。
          </p>

          <!-- 目标音质 -->
          <div class="bdd-field">
            <label class="bdd-label">目标音质</label>
            <select v-model="targetLevel" class="bdd-select" :disabled="loading">
              <option v-for="lvl in LEVELS" :key="lvl.value" :value="lvl.value">
                {{ lvl.label }}
              </option>
            </select>
            <p class="bdd-hint">
              <i class="bi bi-info-circle"></i>
              若目标音质不可用（VIP / 无版权），将自动向下降级直到成功。
            </p>
          </div>

          <!-- 去重开关 -->
          <div class="bdd-field">
            <label class="bdd-checkbox-row">
              <input
                v-model="dedupEnabled"
                type="checkbox"
                class="bdd-checkbox"
                :disabled="loading"
              />
              <span>启用音乐去重</span>
            </label>
            <p class="bdd-hint">
              比较本地已有版本与所选在线音质，按下方策略处理。
            </p>
          </div>

          <!-- 去重策略（仅在启用时显示） -->
          <div v-if="dedupEnabled" class="bdd-field bdd-strategy">
            <label class="bdd-radio-row">
              <input
                v-model="dedupStrategy"
                type="radio"
                value="skip_if_equal"
                :disabled="loading"
              />
              <div class="bdd-radio-text">
                <span class="bdd-radio-title">跳过已存在</span>
                <span class="bdd-radio-desc">
                  本地已有相同或更高音质 → 跳过下载
                </span>
              </div>
            </label>
            <label class="bdd-radio-row">
              <input
                v-model="dedupStrategy"
                type="radio"
                value="replace_if_better"
                :disabled="loading"
              />
              <div class="bdd-radio-text">
                <span class="bdd-radio-title">替换更低音质</span>
                <span class="bdd-radio-desc">
                  本地音质 &lt; 目标 → 下载并删除旧文件；本地音质 ≥ 目标 → 跳过
                </span>
              </div>
            </label>
          </div>
        </div>

        <footer class="bdd-footer">
          <button class="btn btn-secondary" :disabled="loading" @click="close">取消</button>
          <button class="btn btn-primary" :disabled="!canConfirm" @click="confirm">
            <i v-if="loading" class="bi bi-arrow-clockwise bdd-spin"></i>
            <i v-else class="bi bi-cloud-arrow-down"></i>
            {{ loading ? '启动中…' : '开始下载' }}
          </button>
        </footer>
      </div>
    </div>
  </transition>
</template>

<style scoped>
.bdd-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.4);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
  padding: 16px;
}

.bdd-dialog {
  width: min(92vw, 480px);
  background: var(--bg-secondary, #fff);
  border-radius: 14px;
  box-shadow: 0 16px 48px rgba(0, 0, 0, 0.24);
  overflow: hidden;
  display: flex;
  flex-direction: column;
  max-height: 90vh;
}

.bdd-header {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 16px 20px;
  border-bottom: 1px solid var(--border-light, #e8e8e8);
}

.bdd-header > i.bi-cloud-arrow-down {
  font-size: 18px;
  color: var(--primary-color, #0078d7);
}

.bdd-title {
  flex: 1;
  margin: 0;
  font-size: 16px;
  font-weight: 700;
  color: var(--text-primary, #333);
}

.bdd-close {
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
  font-size: 12px;
  transition: background 0.15s;
}

.bdd-close:hover:not(:disabled) {
  background: var(--bg-hover, rgba(0, 0, 0, 0.05));
}

.bdd-close:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.bdd-body {
  padding: 18px 20px;
  display: flex;
  flex-direction: column;
  gap: 18px;
  overflow-y: auto;
}

.bdd-tip {
  margin: 0;
  font-size: 13px;
  color: var(--text-secondary, #666);
  line-height: 1.6;
}

.bdd-tip strong {
  color: var(--primary-color, #0078d7);
  font-weight: 700;
}

.bdd-field {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.bdd-label {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary, #666);
}

.bdd-select {
  width: 100%;
  padding: 8px 12px;
  font-size: 13px;
  border: 1px solid var(--border-color, #e8e8e8);
  border-radius: 8px;
  background: var(--bg-primary, #fff);
  color: var(--text-primary, #333);
  outline: none;
  font-family: inherit;
  box-sizing: border-box;
  cursor: pointer;
}

.bdd-select:focus {
  border-color: var(--primary-color, #0078d7);
}

.bdd-select:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.bdd-hint {
  margin: 2px 0 0;
  font-size: 11px;
  color: var(--text-tertiary, #999);
  display: flex;
  align-items: center;
  gap: 4px;
  line-height: 1.5;
}

.bdd-checkbox-row,
.bdd-radio-row {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  cursor: pointer;
  font-size: 13px;
  color: var(--text-primary, #333);
  margin: 0;
}

.bdd-checkbox-row {
  align-items: center;
  font-weight: 500;
}

.bdd-checkbox,
.bdd-radio-row input[type='radio'] {
  margin: 0;
  margin-top: 2px;
  cursor: pointer;
  accent-color: var(--primary-color, #0078d7);
  flex-shrink: 0;
}

.bdd-checkbox:disabled,
.bdd-radio-row input[type='radio']:disabled {
  cursor: not-allowed;
}

.bdd-strategy {
  padding: 10px 12px;
  background: var(--bg-tertiary, rgba(0, 0, 0, 0.03));
  border-radius: 10px;
  gap: 10px;
}

.bdd-radio-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.bdd-radio-title {
  font-weight: 500;
  color: var(--text-primary, #333);
}

.bdd-radio-desc {
  font-size: 11px;
  color: var(--text-tertiary, #999);
  line-height: 1.5;
}

.bdd-footer {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  padding: 14px 20px;
  border-top: 1px solid var(--border-light, #e8e8e8);
  background: var(--bg-tertiary, rgba(0, 0, 0, 0.02));
}

.btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 8px 16px;
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

.btn-secondary {
  background: var(--bg-secondary, #fff);
}

.bdd-spin {
  animation: bdd-rotate 0.8s linear infinite;
}

@keyframes bdd-rotate {
  to {
    transform: rotate(360deg);
  }
}

/* 过渡动画 */
.bdd-fade-enter-active,
.bdd-fade-leave-active {
  transition: opacity 0.2s;
}

.bdd-fade-enter-active .bdd-dialog,
.bdd-fade-leave-active .bdd-dialog {
  transition: transform 0.2s, opacity 0.2s;
}

.bdd-fade-enter-from,
.bdd-fade-leave-to {
  opacity: 0;
}

.bdd-fade-enter-from .bdd-dialog,
.bdd-fade-leave-to .bdd-dialog {
  transform: translateY(10px) scale(0.96);
  opacity: 0;
}
</style>
