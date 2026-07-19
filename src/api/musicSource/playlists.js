/**
 * 歌单管理 API — 后端 playlist_* 命令
 *
 * 支持本地创建的歌单和从网易云保存的歌单。
 * 歌单内通过 song_ids 引用库内 Song.id。
 */

import { transport } from '@/api/transport';
import { Song } from '@/class';

/**
 * @typedef {Object} Playlist
 * @property {string} id - 库内 UUID
 * @property {string} source - 来源（"netease" / "local"）
 * @property {number|null} remote_id - 在线来源中的歌单 ID
 * @property {string} name
 * @property {string|null} cover_url
 * @property {string|null} creator
 * @property {string|null} description
 * @property {string[]} tags
 * @property {string[]} song_ids - 引用库内 Song.id
 * @property {number} created_at - Unix 毫秒
 * @property {number} updated_at - Unix 毫秒
 */

/** @returns {Promise<Playlist[]>} */
export async function list() {
  return transport.command('playlist_list');
}

/**
 * 获取指定歌单（含完整曲目详情）。
 * @param {string} id
 * @returns {Promise<{ playlist: Playlist, songs: Song[] }|null>}
 */
export async function get(id) {
  const data = await transport.command('playlist_get', { id });
  if (!data) return null;
  return {
    playlist: data.playlist,
    songs: (data.songs || []).map((s) => new Song(s)),
  };
}

/**
 * 创建新的本地空歌单。
 * @param {string} name
 * @param {string} [description]
 * @returns {Promise<Playlist>}
 */
export async function create(name, description = null) {
  return transport.command('playlist_create', { name, description });
}

/**
 * 更新歌单元信息（name / description / tags 等）。
 * @param {Playlist} playlist
 */
export async function update(playlist) {
  return transport.command('playlist_update', { playlist });
}

/**
 * 删除歌单。
 * @param {string} id
 * @returns {Promise<boolean>}
 */
export async function removePlaylist(id) {
  return transport.command('playlist_delete', { id });
}

/**
 * 向歌单追加歌曲（自动去重）。
 * @param {string} playlistId
 * @param {string} songId
 */
export async function addSong(playlistId, songId) {
  return transport.command('playlist_add_song', { playlistId, songId });
}

/**
 * 从歌单移除歌曲。
 * @param {string} playlistId
 * @param {string} songId
 */
export async function removeSong(playlistId, songId) {
  return transport.command('playlist_remove_song', { playlistId, songId });
}

/**
 * 调整歌单内歌曲顺序。
 * @param {string} playlistId
 * @param {string[]} newOrder - 新的 song_id 顺序
 */
export async function reorderSongs(playlistId, newOrder) {
  return transport.command('playlist_reorder_songs', { playlistId, newOrder });
}

/** @returns {Promise<number>} 歌单总数 */
export async function count() {
  return transport.command('playlist_count');
}
