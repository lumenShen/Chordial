/**
 * 歌词优先级 API — 后端 lyric_preference_* / lyric_get_preferred 命令
 *
 * 当一首歌同时存在多个来源的歌词（本地 + 网易云）时，
 * 按用户偏好选出"最佳"歌词用于显示。
 */

import { transport } from '@/api/transport';

/**
 * 歌词格式。
 * @enum {string}
 */
export const LyricFormat = {
  Yrc: 'yrc',
  Qrc: 'qrc',
  Ttml: 'ttml',
  Lrc: 'lrc',
  Plain: 'plain',
};

/**
 * 歌词来源类型。
 * @enum {string}
 */
export const LyricSourceKind = {
  Local: 'local',
  Netease: 'netease',
  Kugou: 'kugou',
  Qq: 'qq',
};

/**
 * 来源标识 → 中文显示名映射（与后端 LyricSourceKind::display_name 一致）。
 */
export const LYRIC_SOURCE_DISPLAY_NAMES = {
  [LyricSourceKind.Local]: '本地',
  [LyricSourceKind.Netease]: '网易云',
  [LyricSourceKind.Kugou]: '酷狗',
  [LyricSourceKind.Qq]: 'QQ 音乐',
};

/**
 * 默认来源优先级顺序（与后端 default_source_order 一致）。
 */
export const DEFAULT_SOURCE_ORDER = [
  LyricSourceKind.Local,
  LyricSourceKind.Netease,
  LyricSourceKind.Kugou,
  LyricSourceKind.Qq,
];

/**
 * @typedef {Object} LyricPreference
 * @property {string[]} source_order - 来源优先级（如 ['local', 'netease', 'kugou', 'qq']）
 * @property {boolean} prefer_word_by_word - 是否优先逐字歌词
 * @property {boolean} fallback_to_online_for_word_by_word - 本地有逐字时是否仍尝试在线获取
 */

/**
 * @typedef {Object} LyricSourceInfo
 * @property {string} kind - LyricSourceKind 值
 * @property {string} source_name - 来源标识（与 kind 相同，与后端 MusicSource::name() 一致）
 * @property {string} display_name - 用户可读名（如 "网易云"）
 * @property {boolean} available - 是否已注册（Local 永远为 true）
 */

/** @returns {Promise<LyricPreference>} */
export async function get() {
  return transport.command('lyric_preference_get');
}

/**
 * 更新歌词优先级配置（自动防抖落盘）。
 * @param {LyricPreference} preference
 */
export async function set(preference) {
  return transport.command('lyric_preference_set', { preference });
}

/**
 * 列出所有可用的歌词来源（用于设置页构建可拖拽优先级列表）。
 * @returns {Promise<LyricSourceInfo[]>}
 */
export async function listSources() {
  const resp = await transport.command('lyric_source_list');
  return resp?.sources ?? [];
}

/**
 * 获取指定歌曲的最佳歌词（综合所有可用来源，按偏好选择）。
 * @param {string} songId
 * @returns {Promise<{
 *   text: string|null,
 *   format: string|null,
 *   source: string|null,
 *   is_word_by_word: boolean,
 *   candidates_count: number,
 * }>}
 */
export async function getPreferred(songId) {
  return transport.command('lyric_get_preferred', { songId });
}

/**
 * 检测歌词文本的格式（前端 heuristic，与后端 LyricFormat::detect 一致）。
 * @param {string} text
 * @returns {string} LyricFormat 值
 */
export function detectFormat(text) {
  if (!text) return LyricFormat.Plain;
  if (text.includes('<p begin=') || text.includes('<?xml')) return LyricFormat.Ttml;
  // YRC: 行首形如 [1234,5678](0,1,0)word
  if (text.split('\n').some((l) => {
    const t = l.trimStart();
    return t.startsWith('[') && t.includes('](');
  })) return LyricFormat.Yrc;
  // QRC: 形如 [1234,5000]word<0,500,0>...
  if (text.includes('<') && text.includes('>') && text.includes(',')) return LyricFormat.Qrc;
  // LRC: 行首形如 [mm:ss.xx]
  if (text.split('\n').some((l) => {
    const t = l.trimStart();
    return t.startsWith('[') && t.length >= 10 && /\d/.test(t[1]);
  })) return LyricFormat.Lrc;
  return LyricFormat.Plain;
}

/**
 * 是否为逐字格式。
 * @param {string} format
 * @returns {boolean}
 */
export function isWordByWord(format) {
  return format === LyricFormat.Yrc
    || format === LyricFormat.Qrc
    || format === LyricFormat.Ttml;
}
