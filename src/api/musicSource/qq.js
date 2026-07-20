/**
 * QQ 音乐来源 API — 后端 qq_* 命令
 *
 * 通过 QQMusicApi 自建/公共 API 服务器访问 QQ 音乐资源。
 * 用户需在设置页配置 base_url 和 cookie 后才能使用。
 *
 * QQ 音质字符串与内部 AudioQuality 等级的对应关系：
 * - "128"  → Standard（标准 128 kbps MP3）
 * - "m4a"  → Higher（较高 AAC，码率通常 < 128）
 * - "320"  → Exhigh（极高 320 kbps MP3，默认）
 * - "ape"  → Lossless（无损 APE）
 * - "flac" → Lossless（无损 FLAC）
 */

import { transport } from '@/api/transport';
import { Song, Artist, Album } from '@/class';

// ══════════════════════════════════════════════════════════════════════════════
// 配置管理
// ══════════════════════════════════════════════════════════════════════════════

/**
 * QQ 来源配置（与 netease/kugou 配置结构一致，共用 WebSourceConfig）。
 * @typedef {Object} QqConfig
 * @property {string} base_url - API 基础 URL（如 https://qq-api.example.com）
 * @property {string} cookie - 登录 cookie（可空，含 uin/qqmusic_key 等）
 * @property {number} timeout_secs - 请求超时秒数（0 = 默认 15s）
 * @property {boolean} enabled - 是否启用此来源
 * @property {string} download_dir - 下载目录（空则用应用默认目录）
 */

/** @returns {Promise<QqConfig>} */
export async function getConfig() {
  return transport.command('qq_get_config');
}

/**
 * 更新配置并持久化。
 * @param {QqConfig} config
 */
export async function updateConfig(config) {
  return transport.command('qq_update_config', { config });
}

/**
 * 测试与 API 服务器的连通性。
 * @returns {Promise<{ ok: boolean, error: string|null, base_url: string }>}
 */
export async function testConnection() {
  return transport.command('qq_test_connection');
}

// ══════════════════════════════════════════════════════════════════════════════
// 搜索
// ══════════════════════════════════════════════════════════════════════════════

/**
 * 在线搜索 QQ 歌曲（返回库内 Song 模型，附带 qq SourceId）。
 *
 * QQ 歌曲的 entity_id 格式为 `song:<songmid>`（字符串标识），
 * 下载时通过 source_download_song 命令传入此 entity_id。
 *
 * @param {string} query - 搜索关键词
 * @returns {Promise<Song[]>}
 */
export async function searchSongs(query) {
  const data = await transport.command('qq_search_songs', { query });
  return (data || []).map((d) => new Song(d));
}

/**
 * 在线搜索 QQ 歌手（返回库内 Artist 模型，附带 qq SourceId）。
 * @param {string} query - 搜索关键词
 * @param {number} [limit=30]
 * @param {number} [offset=0]
 * @returns {Promise<Artist[]>}
 */
export async function searchArtists(query, limit = 30, offset = 0) {
  const data = await transport.command('qq_search_artists', { query, limit, offset });
  return Artist.fromDataArray(data || []);
}

/**
 * 在线搜索 QQ 专辑（返回库内 Album 模型，附带 qq SourceId）。
 * @param {string} query - 搜索关键词
 * @param {number} [limit=30]
 * @param {number} [offset=0]
 * @returns {Promise<Album[]>}
 */
export async function searchAlbums(query, limit = 30, offset = 0) {
  const data = await transport.command('qq_search_albums', { query, limit, offset });
  return Album.fromDataArray(data || []);
}
