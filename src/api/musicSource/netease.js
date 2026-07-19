/**
 * 网易云音乐来源 API — 后端 netease_* 命令
 *
 * 通过 NeteaseCloudMusicApi 自建/公共 API 服务器访问网易云音乐资源。
 * 用户需在设置页配置 base_url 和 cookie 后才能使用。
 */

import { transport } from '@/api/transport';
import { Song } from '@/class';

// ══════════════════════════════════════════════════════════════════════════════
// 配置管理
// ══════════════════════════════════════════════════════════════════════════════

/**
 * 网易云来源配置。
 * @typedef {Object} NeteaseConfig
 * @property {string} base_url - API 基础 URL（如 https://ncm-api.example.com）
 * @property {string} cookie - 登录 cookie（可空）
 * @property {number} timeout_secs - 请求超时秒数（0 = 默认 15s）
 * @property {boolean} enabled - 是否启用此来源
 * @property {string} download_dir - 下载目录（空则用应用默认目录）
 */

/** @returns {Promise<NeteaseConfig>} */
export async function getConfig() {
  return transport.command('netease_get_config');
}

/**
 * 更新配置并持久化。
 * @param {NeteaseConfig} config
 */
export async function updateConfig(config) {
  return transport.command('netease_update_config', { config });
}

/**
 * 测试与 API 服务器的连通性。
 * @returns {Promise<{ ok: boolean, error: string|null, base_url: string }>}
 */
export async function testConnection() {
  return transport.command('netease_test_connection');
}

// ══════════════════════════════════════════════════════════════════════════════
// 搜索
// ══════════════════════════════════════════════════════════════════════════════

/**
 * 在线搜索歌曲（返回库内 Song 模型，附带 netease SourceId）。
 * @param {string} query - 搜索关键词
 * @returns {Promise<Song[]>}
 */
export async function searchSongs(query) {
  const data = await transport.command('netease_search_songs', { query });
  return (data || []).map((d) => new Song(d));
}

/**
 * 在线搜索歌单。
 * @param {string} query - 搜索关键词
 * @param {number} [limit=30]
 * @param {number} [offset=0]
 * @returns {Promise<NeteasePlaylistBrief[]>}
 */
export async function searchPlaylists(query, limit = 30, offset = 0) {
  return transport.command('netease_search_playlists', { query, limit, offset });
}

/**
 * @typedef {Object} NeteasePlaylistBrief
 * @property {number} id
 * @property {string} name
 * @property {string|null} coverImgUrl
 * @property {{ nickname: string, userId: number }|null} creator
 * @property {number} trackCount
 * @property {number} playCount
 * @property {string|null} description
 */

/**
 * @typedef {Object} NeteasePlaylistDetail
 * @property {number} id
 * @property {string} name
 * @property {string|null} coverImgUrl
 * @property {{ nickname: string, userId: number }|null} creator
 * @property {number} trackCount
 * @property {string|null} description
 * @property {string[]} tags
 */

/**
 * @typedef {Object} NeteaseSongDetail
 * @property {number} id
 * @property {string} name
 * @property {NeteaseArtistBrief[]} ar
 * @property {NeteaseAlbumBrief|null} al
 * @property {number} dt
 * @property {number|null} size
 */

/**
 * @typedef {Object} NeteaseArtistBrief
 * @property {number} id
 * @property {string} name
 * @property {string|null} img1v1Url
 * @property {string|null} picUrl
 */

/**
 * @typedef {Object} NeteaseAlbumBrief
 * @property {number} id
 * @property {string} name
 * @property {string|null} picUrl
 */

/**
 * 获取歌单详情（元信息 + 全部曲目）。
 * @param {number} playlistId
 * @returns {Promise<{ detail: NeteasePlaylistDetail, tracks: NeteaseSongDetail[] }>}
 */
export async function getPlaylistFull(playlistId) {
  return transport.command('netease_get_playlist_full', { playlistId });
}

// ══════════════════════════════════════════════════════════════════════════════
// 歌单保存到本地
// ══════════════════════════════════════════════════════════════════════════════

/**
 * 将在线歌单保存为本地 Playlist，并把所有曲目加入音乐库（自动去重）。
 * @param {number} playlistId - 网易云歌单 ID
 * @returns {Promise<import('./playlists.js').Playlist>}
 */
export async function savePlaylistToLibrary(playlistId) {
  return transport.command('netease_save_playlist_to_library', { playlistId });
}

// ══════════════════════════════════════════════════════════════════════════════
// 下载与音质比较
// ══════════════════════════════════════════════════════════════════════════════

/**
 * 下载网易云歌曲到本地 download_dir。
 * @param {string} songEntityId - 网易云 entity_id（如 "song:123456"）
 * @param {string} title - 歌曲标题（用于文件命名）
 * @param {string} artist - 主艺术家名（用于文件命名）
 * @param {string} [level='exhigh'] - 音质等级
 * @returns {Promise<string>} 下载后的本地文件绝对路径
 */
export async function downloadSong(songEntityId, title, artist, level = 'exhigh') {
  return transport.command('netease_download_song', {
    songEntityId,
    title,
    artist,
    level,
  });
}

/**
 * 比较 online 歌曲与本地库中相同歌曲的音质。
 * @param {string} title
 * @param {string} artist
 * @param {string} [album]
 * @returns {Promise<{
 *   matches: Array<{
 *     song_id: string,
 *     title: string,
 *     artist_names: string[],
 *     album_title: string|null,
 *     file_path: string|null,
 *     quality: string,
 *     quality_rank: number,
 *   }>,
 *   has_local_match: boolean,
 * }>}
 */
export async function compareWithLocal(title, artist, album = null) {
  return transport.command('netease_compare_with_local', { title, artist, album });
}

// ══════════════════════════════════════════════════════════════════════════════
// 扫码登录
// ══════════════════════════════════════════════════════════════════════════════

/**
 * 第 1 步：获取扫码登录的 unikey。
 * @returns {Promise<string>} 32 位 unikey
 */
export async function loginQrKey() {
  return transport.command('netease_login_qr_key');
}

/**
 * 第 2 步：用 unikey 生成二维码。
 * @param {string} key - 从 loginQrKey 获取的 unikey
 * @param {boolean} [qrimg=true] - 是否返回 base64 data URL
 * @returns {Promise<{ qrurl: string, qrimg: string|null }>}
 */
export async function loginQrCreate(key, qrimg = true) {
  return transport.command('netease_login_qr_create', { key, qrimg });
}

/**
 * 第 3 步：轮询扫码状态。
 *
 * status 取值：
 * - `expired`: 二维码已过期（code 800）
 * - `waiting`: 等待扫码（code 801）
 * - `scanned`: 已扫码待确认（code 802）
 * - `authorized`: 授权成功（code 803，后端已自动持久化 cookie）
 * - `unknown`: 未知状态
 *
 * @param {string} key
 * @returns {Promise<{
 *   code: number,
 *   status: 'expired'|'waiting'|'scanned'|'authorized'|'unknown',
 *   message: string|null,
 *   avatarUrl: string|null,
 *   nickname: string|null,
 *   cookie: string|null,
 * }>}
 */
export async function loginQrCheck(key) {
  return transport.command('netease_login_qr_check', { key });
}

// ══════════════════════════════════════════════════════════════════════════════
// 用户账户与"我的资源"（依赖登录 cookie）
// ══════════════════════════════════════════════════════════════════════════════

/**
 * 获取当前登录账号信息。
 * @returns {Promise<{
 *   code: number,
 *   profile: { userId: number, nickname: string, avatarUrl: string|null, signature: string|null } | null,
 *   account: { id: number, userName: string } | null,
 * }>}
 */
export async function getUserAccount() {
  return transport.command('netease_get_user_account');
}

/**
 * 获取指定用户的全部歌单（创建 + 收藏）。
 * @param {number} uid - 用户 ID（从 getUserAccount 获取）
 * @returns {Promise<Array<{
 *   id: number, name: string, coverImgUrl: string|null,
 *   creator: { nickname: string, userId: number }|null,
 *   trackCount: number, playCount: number, description: string|null,
 *   subscribed: boolean|null, tags: string[],
 * }>>}
 */
export async function getUserPlaylists(uid) {
  return transport.command('netease_get_user_playlists', { uid });
}

/**
 * 获取当前用户收藏的专辑（分页）。
 * @param {number} [limit=50]
 * @param {number} [offset=0]
 * @returns {Promise<{
 *   albums: Array<{
 *     id: number, name: string, picUrl: string|null,
 *     artist: object|null, artists: array,
 *     trackCount: number, playCount: number, subTime: number,
 *   }>,
 *   total: number, offset: number, limit: number, hasMore: boolean,
 * }>}
 */
export async function getUserAlbums(limit = 50, offset = 0) {
  return transport.command('netease_get_user_albums', { limit, offset });
}

/**
 * 将网易云专辑导入本地音乐库（把专辑所有曲目加入 library，不创建本地 Playlist）。
 * @param {number} albumId
 * @returns {Promise<{
 *   album: { id, name, picUrl, artist, artists, description, publishTime, size },
 *   imported_count: number,
 *   total_count: number,
 * }>}
 */
export async function saveAlbumToLibrary(albumId) {
  return transport.command('netease_save_album_to_library', { albumId });
}
