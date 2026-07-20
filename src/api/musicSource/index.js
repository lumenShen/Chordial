/**
 * 音乐源 API 统一导出
 *
 * 注意：netease / kugou / lyricPreference 通过命名空间导出，
 * 避免与 library.js 的同名函数（如 searchSongs / searchLyrics）冲突。
 */
export * from './sources.js';
export * from './library.js';
export * from './cache.js';
export * from './musicResource.js';
export * from './resourceLoader.js';
export * from './playlists.js';
export * as netease from './netease.js';
export * as kugou from './kugou.js';
export * as lyricPreference from './lyricPreference.js';
