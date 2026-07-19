/**
 * 音乐源管理 API — 后端 local_* 命令
 *
 * 当前仅支持本地来源（LocalMusicSource），网盘来源尚未实现。
 */

import { transport } from '@/api/transport';

// ══════════════════════════════════════════════════════════════════════════════
// Local Folder Management
// ══════════════════════════════════════════════════════════════════════════════

/**
 * 添加本地音乐文件夹。
 *
 * 后端操作：
 * 1. 持久化文件夹路径
 * 2. 全量扫描音频文件并导入 MusicLibrary
 * 3. 启动文件系统监听
 *
 * @param {string} path - 文件夹绝对路径
 * @returns {Promise<{added: boolean, path: string, files_found: number, indexed: number, errors: string[]}>}
 */
export async function addLocalFolder(path) {
  return transport.command('local_add_folder', { path });
}

/**
 * 移除本地音乐文件夹。
 *
 * 后端操作：
 * 1. 注销文件夹下所有音频文件的 SourceId（级联清理空实体）
 * 2. 从文件夹管理器移除
 * 3. 持久化
 *
 * @param {string} path - 文件夹绝对路径
 * @returns {Promise<{removed: boolean, path: string, cleaned_files: number}>}
 */
export async function removeLocalFolder(path) {
  return transport.command('local_remove_folder', { path });
}

/** 别名 —— 保持向后兼容 */
export { removeLocalFolder as remove };

/**
 * 获取已添加的本地文件夹列表。
 * @returns {Promise<string[]>} 文件夹路径数组
 */
export async function getFolders() {
  return transport.command('local_get_folders');
}

/** 别名 —— 保持向后兼容 */
export { getFolders as getAll };

/**
 * 获取本地来源索引统计。
 * @returns {Promise<{folder_count: number, indexed_files: number}>}
 */
export async function getLocalStats() {
  return transport.command('local_stats');
}

/**
 * 手动重新扫描所有文件夹（调试用）。
 * @returns {Promise<{indexed: number, folders_scanned: number}>}
 */
export async function rescanAll() {
  return transport.command('local_rescan');
}

// ══════════════════════════════════════════════════════════════════════════════
// Source enable / disable — 暂未实现
// ══════════════════════════════════════════════════════════════════════════════

/**
 * 设置源启用状态（当前 stub，后端尚未实现非本地来源开关）。
 * @deprecated
 */
export async function setEnabled(_id, _enabled) {
  throw new Error('setEnabled 暂未实现 — 本地来源是 must-source，不允许禁用');
}

/**
 * 添加网盘来源（后端尚未实现）。
 * @deprecated 后端未实现网盘源
 */
export async function addWebDisk(_url, _username, _password) {
  throw new Error('网盘来源后端尚未实现');
}

/**
 * 添加 WebDAV 来源（后端尚未实现）。
 * @deprecated 后端未实现 WebDAV 源
 */
export async function addWebDev(_apiBaseUrl, _name, _apiKey, _authToken) {
  throw new Error('WebDAV 来源后端尚未实现');
}

// ══════════════════════════════════════════════════════════════════════════════
// 通用云端来源 — 从网络来源实例选择一个云端音乐源进行下载/比较
// ══════════════════════════════════════════════════════════════════════════════

/**
 * @typedef {Object} WebSourceInfo
 * @property {string} name - 来源名（如 `"netease"`）
 * @property {string} display_name - 显示名（如 `"网易云音乐"`）
 * @property {string} source_type - 来源内部类型标识
 * @property {boolean} enabled - 是否已启用
 */

/**
 * @typedef {Object} AudioQualityDescriptor
 * @property {number} rank - 归一化等级（0-5）
 * @property {number} bitrate_kbps - 码率（kbps）
 * @property {string} codec - 编码格式（"mp3"/"flac"/"aac"/...）
 * @property {number|null} sample_rate_hz - 采样率（Hz）
 * @property {number|null} bit_depth - 位深
 */

/**
 * 列出已注册的云端（Web）音乐来源。
 *
 * 用于下载对话框的"来源选择"下拉框。
 * 仅返回已通过 registrar.register 注册的 Web 类型来源。
 * @returns {Promise<WebSourceInfo[]>}
 */
export async function listWebSources() {
  return transport.command('source_list_web_sources');
}

/**
 * 从指定云端来源下载歌曲到本地 download_dir。
 *
 * @param {string} sourceName - 来源名（如 `"netease"`）
 * @param {string} songEntityId - 来源内部的歌曲 entity_id（如 `"song:123456"`）
 * @param {string} title - 歌曲标题（用于文件命名）
 * @param {string} artist - 主艺术家名（用于文件命名）
 * @param {string} [level] - 音质等级（默认 `"exhigh"`）
 * @returns {Promise<{
 *   path: string,
 *   quality_descriptor: AudioQualityDescriptor,
 *   quality_label: string,
 * }>}
 */
export async function downloadFromSource(sourceName, songEntityId, title, artist, level = 'exhigh') {
  return transport.command('source_download_song', {
    sourceName,
    songEntityId,
    title,
    artist,
    level,
  });
}

/**
 * 比较 online 歌曲与本地库中相同歌曲的音质（通用版本）。
 *
 * @param {string} sourceName - 来源名
 * @param {string} title - 歌曲标题
 * @param {string} artist - 艺术家名
 * @param {string|null} [album] - 专辑名（可选）
 * @returns {Promise<{
 *   matches: Array<{
 *     song_id: string,
 *     title: string,
 *     artist_names: string[],
 *     album_title: string|null,
 *     file_path: string|null,
 *     quality: string,
 *     quality_rank: number,
 *     quality_descriptor: AudioQualityDescriptor,
 *     quality_label: string,
 *   }>,
 *   has_local_match: boolean,
 * }>}
 */
export async function compareWithLocalFromSource(sourceName, title, artist, album = null) {
  return transport.command('source_compare_with_local', {
    sourceName,
    title,
    artist,
    album,
  });
}

// ══════════════════════════════════════════════════════════════════════════════
// 数据维护 — 设置页"清理数据"工具
// ══════════════════════════════════════════════════════════════════════════════

/**
 * 获取数据维护统计信息（计数 + 路径 + 磁盘占用）。
 * @returns {Promise<{
 *   counts: { songs: number, artists: number, albums: number, lyrics: number, folders: number, indexed_files: number },
 *   paths: { library: string, storage: string, config: string, folder_store: string },
 *   disk_usage_bytes: number,
 * }>}
 */
export async function maintenanceStats() {
  return transport.command('maintenance_stats');
}

/**
 * 清空音乐库（Song / Artist / Album / Lyric）+ 本地索引 + 文件夹列表。
 * 保留：配置、通用存储、缓存。
 * @returns {Promise<{ cleared: boolean, message: string }>}
 */
export async function maintenanceClearLibrary() {
  return transport.command('maintenance_clear_library');
}

/**
 * 清空内存缓存 + Blob 磁盘缓存。不影响音乐库 / 配置 / 存储。
 * @returns {Promise<{ cleared: boolean, memory_items: number, blob_items: number }>}
 */
export async function maintenanceClearCache() {
  return transport.command('maintenance_clear_cache');
}

/**
 * 清空通用存储（storage.json）的所有键值。不影响音乐库 / 配置 / 缓存。
 * @returns {Promise<{ cleared: boolean, items: number }>}
 */
export async function maintenanceClearStorage() {
  return transport.command('maintenance_clear_storage');
}

/**
 * 重置全部数据 — 音乐库 + 本地索引 + 文件夹 + 配置 + 存储 + 缓存。
 * 等效于"恢复出厂设置"。调用后建议重启应用。
 * @returns {Promise<{ reset: boolean, message: string }>}
 */
export async function maintenanceResetAll() {
  return transport.command('maintenance_reset_all');
}

/**
 * 修复库中 song.artist_ids / song.album_id 引用的无效 UUID。
 * 适用场景：点击歌手详情页时报"歌手不存在 / 找不到该歌手的信息"。
 * 修复后无需重新扫描即可正常访问歌手/专辑页面。
 * @returns {Promise<{ repaired: boolean, repaired_songs: number, repaired_artists: number, repaired_albums: number, message: string }>}
 */
export async function maintenanceRepairReferences() {
  return transport.command('maintenance_repair_references');
}

// ══════════════════════════════════════════════════════════════════════════════
// 诊断命令 — 歌词问题排查
// ══════════════════════════════════════════════════════════════════════════════

/**
 * 诊断指定歌曲的歌词状态。
 *
 * 用于排查"部分歌曲歌词无法显示"问题。一次性返回：
 * - 库中 Song 实体（含 source_ids、lyric_id）
 * - 库中 Lyric 实体（按 song_id 查，可能为 null）
 * - 实际音频文件路径 + 是否存在
 * - 同目录 .lrc/.txt 文件内容预览
 * - symphonia 探测到的所有原始标签（key + value 类型）
 * - scanner.probe_file 提取出的嵌入歌词预览
 *
 * 用法（在浏览器控制台）：
 * ```js
 * import('@/api/musicSource/sources.js').then(m => m.debugInspectSongLyrics('<songId>')).then(console.log)
 * ```
 *
 * @param {string} songId - 库内 Song ID
 * @returns {Promise<object>}
 */
export async function debugInspectSongLyrics(songId) {
  return transport.command('debug_inspect_song_lyrics', { songId });
}
