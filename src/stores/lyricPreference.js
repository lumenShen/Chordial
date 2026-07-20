/**
 * LyricPreferenceStore — 歌词优先级配置的响应式状态中心。
 *
 * # 设计
 * - 集中管理 `source_order` / `prefer_word_by_word` / `fallback_to_online_for_word_by_word`
 * - 通过后端 `lyric_preference_get` / `lyric_preference_set` 命令持久化到 config.json
 * - 用户修改时自动防抖（500ms）落盘，避免高频 IPC 调用
 * - 同时维护 `availableSources`（来自 `lyric_source_list`），供设置页构建可拖拽列表
 *
 * # 与 AmllSettingsStore 的区别
 * AmllSettingsStore 用 localStorage（仅前端配置，与 AMLL Jotai atom 双向绑定）。
 * 本 store 走后端 config.json，因为 `lyric_get_preferred` 等后端命令需要读取此配置。
 */
import { reactive, watch } from 'vue';
import {
  get as loadPreference,
  set as savePreference,
  listSources,
  DEFAULT_SOURCE_ORDER,
} from '@/api/musicSource/lyricPreference.js';

// ── 默认值（与后端 LyricPreference::default() 对齐）──────────
const DEFAULTS = {
  source_order: [...DEFAULT_SOURCE_ORDER],
  prefer_word_by_word: true,
  fallback_to_online_for_word_by_word: false,
};

// ── 响应式状态 ───────────────────────────────────────────
const state = reactive({
  ...DEFAULTS,
});

// 可用歌词来源列表（Local 永远可用，其他依赖 registrar 注册状态）
const availableSources = reactive([]);

// ── 内部状态 ─────────────────────────────────────────────
let _initialized = false;
let _saveTimer = null;
const SAVE_DEBOUNCE_MS = 500;

/**
 * 持久化当前 state 到后端（防抖）。
 */
function scheduleSave() {
  if (_saveTimer) clearTimeout(_saveTimer);
  _saveTimer = setTimeout(async () => {
    _saveTimer = null;
    try {
      await savePreference({
        source_order: [...state.source_order],
        prefer_word_by_word: state.prefer_word_by_word,
        fallback_to_online_for_word_by_word: state.fallback_to_online_for_word_by_word,
      });
    } catch (e) {
      console.warn('[LyricPreferenceStore] 保存失败:', e);
    }
  }, SAVE_DEBOUNCE_MS);
}

/**
 * 初始化：从后端加载配置 + 可用来源列表，并启动 watch 自动持久化。
 *
 * 在应用启动时调用一次（main.js 或首次进入设置页时懒加载均可）。
 */
export async function initLyricPreference() {
  if (_initialized) return;
  _initialized = true;

  // 并行加载配置 + 可用来源
  const [pref, sources] = await Promise.all([
    loadPreference().catch((e) => {
      console.warn('[LyricPreferenceStore] 加载配置失败:', e);
      return null;
    }),
    listSources().catch((e) => {
      console.warn('[LyricPreferenceStore] 加载来源列表失败:', e);
      return [];
    }),
  ]);

  if (pref) {
    state.source_order = [...(pref.source_order ?? DEFAULTS.source_order)];
    state.prefer_word_by_word = pref.prefer_word_by_word ?? DEFAULTS.prefer_word_by_word;
    state.fallback_to_online_for_word_by_word =
      pref.fallback_to_online_for_word_by_word ?? DEFAULTS.fallback_to_online_for_word_by_word;
  }

  availableSources.splice(0, availableSources.length, ...(sources || []));

  // 启动 watch：state 变化时自动持久化（深 watch，source_order 数组内部变化也触发）
  watch(
    () => ({
      source_order: [...state.source_order],
      prefer_word_by_word: state.prefer_word_by_word,
      fallback_to_online_for_word_by_word: state.fallback_to_online_for_word_by_word,
    }),
    scheduleSave,
    { deep: true },
  );
}

/**
 * 强制刷新可用来源列表（在来源注册状态变化后调用，如启用/禁用某个音乐源后）。
 */
export async function refreshAvailableSources() {
  const sources = await listSources().catch(() => []);
  availableSources.splice(0, availableSources.length, ...(sources || []));
}

/**
 * 重置为默认配置。
 */
export function resetToDefaults() {
  state.source_order = [...DEFAULTS.source_order];
  state.prefer_word_by_word = DEFAULTS.prefer_word_by_word;
  state.fallback_to_online_for_word_by_word = DEFAULTS.fallback_to_online_for_word_by_word;
  // 重置后立即触发 watch → scheduleSave
}

/**
 * 上移某个来源（在 source_order 中的位置 -1）。
 * @param {string} kind - LyricSourceKind 值
 */
export function moveSourceUp(kind) {
  const idx = state.source_order.indexOf(kind);
  if (idx <= 0) return;
  const arr = [...state.source_order];
  [arr[idx - 1], arr[idx]] = [arr[idx], arr[idx - 1]];
  state.source_order = arr;
}

/**
 * 下移某个来源（在 source_order 中的位置 +1）。
 * @param {string} kind - LyricSourceKind 值
 */
export function moveSourceDown(kind) {
  const idx = state.source_order.indexOf(kind);
  if (idx < 0 || idx >= state.source_order.length - 1) return;
  const arr = [...state.source_order];
  [arr[idx + 1], arr[idx]] = [arr[idx], arr[idx + 1]];
  state.source_order = arr;
}

/**
 * 将某个来源移动到指定位置（用于拖拽排序）。
 * @param {string} kind - LyricSourceKind 值
 * @param {number} toIndex - 目标位置
 */
export function moveSourceTo(kind, toIndex) {
  const fromIndex = state.source_order.indexOf(kind);
  if (fromIndex < 0 || toIndex < 0 || toIndex >= state.source_order.length) return;
  if (fromIndex === toIndex) return;
  const arr = [...state.source_order];
  const [item] = arr.splice(fromIndex, 1);
  arr.splice(toIndex, 0, item);
  state.source_order = arr;
}

export const LyricPreferenceStore = {
  state,
  availableSources,
  init: initLyricPreference,
  refreshAvailableSources,
  resetToDefaults,
  moveSourceUp,
  moveSourceDown,
  moveSourceTo,
};

export default LyricPreferenceStore;
