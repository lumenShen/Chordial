/**
 * Song / Track — 歌曲
 *
 * 对应后端 `music_library::models::Song`。
 * 别名 `Track` 用于兼容现有组件。
 */
import { SourceId } from './SourceId.js';

export class Song {
  /** @param {Partial<Song>} data 后端 JSON 或普通对象 */
  constructor(data = {}) {
    /** 库内统一 ID */
    this.id = data.id ?? '';
    /** 歌曲标题 */
    this.title = data.title ?? '未知歌曲';
    /** 艺人名称列表 */
    this.artistNames = data.artist_names ?? data.artistNames ?? [];
    /** 专辑名称 */
    this.albumTitle = data.album_title ?? data.albumTitle ?? null;
    /** 时长（秒） */
    this.duration = data.duration ?? null;
    /** 关联的艺术家 ID 列表 */
    this.artistIds = data.artist_ids ?? data.artistIds ?? [];
    /** 关联的专辑 ID */
    this.albumId = data.album_id ?? data.albumId ?? null;
    /** 关联的歌词 ID */
    this.lyricId = data.lyric_id ?? data.lyricId ?? null;
    /** 来源引用列表 */
    this.sourceIds = (data.source_ids ?? data.sourceIds ?? []).map(
      (s) => (s instanceof SourceId ? s : new SourceId(s)),
    );
    /** 封面图片 URL（网络来源直接提供，优先于 sourceIds 的 chordial:// 协议） */
    this.coverUrl = data.cover_url ?? data.coverUrl ?? null;
  }

  // ── 显示辅助 ────────────────────────────────────

  /** 艺人名称（逗号连接） */
  get artist() {
    return this.artistNames.length > 0
      ? this.artistNames.join(', ')
      : '未知歌手';
  }

  getDisplayArtist() {
    return this.artist;
  }

  /** 主要艺人，如果有 */
  get primaryArtist() {
    if (this.artistIds.length > 0) {
      return {
        id: this.artistIds[0],
        name: this.artistNames[0] ?? '未知歌手',
      };
    }
    return null;
  }

  // ── 资源方法 ────────────────────────────────────

  /**
   * 获取音频 URL（通过 chordial:// 自定义协议流式传输）。
   *
   * 直接返回 chordial:// URL，浏览器原生处理流式传输和 Range 请求，
   * 无需 invoke + Blob + createObjectURL。
   * @returns {Promise<string|null>}
   */
  async getAudioBlobUrl() {
    if (!this.sourceIds || this.sourceIds.length === 0) return null;
    const { buildAudioUrl } = await import('@/api/musicSource/chordialUrl.js');
    try {
      return buildAudioUrl(this.sourceIds[0]);
    } catch (e) {
      console.warn('构建音频 URL 失败:', e);
      return null;
    }
  }

  /**
   * 释放音频资源（chordial:// 协议下为 no-op，保留接口兼容性）。
   */
  releaseAudio() {
    // chordial:// 协议下无需手动释放，浏览器按需缓存管理
  }

  /**
   * 获取封面资源 URL（通过 chordial:// 自定义协议）。
   * @param {string} _size - 图片尺寸（本地来源忽略）
   * @returns {Promise<{url: string, release: Function}|null>}
   */
  async acquireCoverResource(_size = 'medium') {
    // 优先级1: 网络来源直接提供的 coverUrl（避免 chordial:// 协议二次拉取）
    // 网易云搜索/详情接口已返回 al.picUrl，直接用 URL 不走 /album 接口
    if (this.coverUrl) {
      return { url: this.coverUrl, release: () => {} };
    }

    // 优先级2: 通过 chordial:// 协议从本地音频标签提取封面
    if (this.sourceIds && this.sourceIds.length > 0) {
      try {
        const { buildImageUrl } = await import('@/api/musicSource/chordialUrl.js');
        const url = buildImageUrl(this.sourceIds[0]);
        return { url, release: () => {} };
      } catch (e) {
        console.warn('获取歌曲封面失败:', e);
      }
    }
    return null;
  }

  /**
   * 获取歌词信息（同步/异步兼容，优先从缓存）
   * @returns {Promise<{plainLyrics: string, syncedLyrics: string, hasSyncedLyrics: boolean, hasPlainLyrics: boolean}>}
   */
  async getLyricsInfo() {
    const result = {
      plainLyrics: '',
      syncedLyrics: '',
      hasSyncedLyrics: false,
      hasPlainLyrics: false
    };

    try {
      let lyricText = '';

      // 优先级1：库内 Lyric 实体（已入库的歌曲，如本地扫描或导入的网易云曲目）
      // 始终按 song_id 查询；lyricId 字段仅为元数据，不可作为查询门控。
      // 旧实现 `if (this.lyricId)` 会跳过 lyric_id 为 null 的歌曲，
      // 但 Lyric 实体可能已存在于库中（按 song_id 索引），
      // 导致已扫描歌曲的歌词无法显示。
      // 后端 getLyricOfSong 用 JSON 层字段匹配，~0.1ms，无性能担忧。
      const { getLyricOfSong } = await import('@/api/musicSource/library.js');
      const lyric = await getLyricOfSong(this.id);

      // ── 诊断日志：追踪歌词数据流向 ──
      console.log('[Song.getLyricsInfo] songId=%s title=%s', this.id, this.title, {
        lyricFound: !!lyric,
        lyricId: lyric?.id ?? null,
        lyricTextLen: lyric?.text?.length ?? 0,
        lyricTextPreview: lyric?.text ? lyric.text.slice(0, 80) : null,
        hasSourceId: !!lyric?.sourceId,
      });

      if (lyric && lyric.text) {
        lyricText = lyric.text;
      } else if (lyric && lyric.sourceId) {
        // 库内 Lyric 实体存在但 text 为空，通过 sourceId 重新拉取
        const { getLyricText } = await import('@/api/musicSource/musicResource.js');
        lyricText = await getLyricText(lyric.sourceId) || '';
        console.log('[Song.getLyricsInfo] 通过库内 Lyric.sourceId 回查歌词: len=%d', lyricText.length);
      }

      // 优先级2：在线来源直拉歌词（未入库的在线搜索歌曲）
      // 场景：用户在搜索页点击网易云歌曲播放，歌曲尚未入库，
      // 库内无 Lyric 实体，需要通过歌曲自身的 SourceId 调用 getLyricText
      // 后端 resource::get_lyric_text → source.lyric_text_get(entity_id) → 网易云 /lyric API
      if (!lyricText) {
        const neteaseSid = this.sourceIds?.find(
          (s) => s?.sourceName === 'netease' && s?.entityType === 'Song' && s?.entityId,
        );
        if (neteaseSid) {
          try {
            const { getLyricText } = await import('@/api/musicSource/musicResource.js');
            lyricText = (await getLyricText(neteaseSid)) || '';
            console.log(
              '[Song.getLyricsInfo] 通过歌曲 SourceId 在线拉取网易云歌词: len=%d, entityId=%s',
              lyricText.length,
              neteaseSid.entityId,
            );
          } catch (e) {
            console.warn('[Song.getLyricsInfo] 在线拉取网易云歌词失败:', e);
          }
        }
      }

      if (!lyricText) {
        console.warn('[Song.getLyricsInfo] 最终无歌词文本，返回空 result. songId=%s', this.id);
        return result;
      }

      // 检测是否为同步歌词
      // 直接把原始文本赋给 syncedLyrics，由 useAmllBridge 的 parseToAmllLyricLines
      // 用 parseLyrics (lyricConverter.js) 统一解析 LRC/YRC/QRC/TTML。
      // 不再做 parseSyncedLyrics → formatToLRC 往返转换：
      //   1. 往返是有损的（formatToLRC 丢精度、丢元数据行）
      //   2. parseSyncedLyrics 的 matchAll 缺 g 标志会抛 TypeError 被静默吃掉
      //   3. useAmllBridge 本来就要重新解析，parse→format→parse 是浪费
      //
      // 格式检测（与 lyricConverter.js parseLyrics 的自动检测保持一致）：
      //   - YRC: [16210,3460](16210,670,0)还(16880,410,0)没...（网易云逐字）
      //   - LRC: [mm:ss.xx]歌词（行级同步）
      //   - 其他: 纯文本
      const trimmedText = lyricText.trim();
      const isYrc = /\[\d+,\d+\]\(\d+,\d+,\d+\)/.test(trimmedText);
      const isLrc = /^\[(\d{1,2}):(\d{2})\.(\d{2,3})\]/.test(trimmedText);
      if (isYrc || isLrc) {
        result.syncedLyrics = lyricText;
        result.hasSyncedLyrics = true;
        console.log(
          '[Song.getLyricsInfo] 同步歌词(%s), len=%d',
          isYrc ? 'YRC' : 'LRC',
          lyricText.length,
        );
      } else {
        result.plainLyrics = lyricText;
        result.hasPlainLyrics = true;
        console.log('[Song.getLyricsInfo] 纯文本歌词, len=%d', lyricText.length);
      }

      return result;
    } catch (error) {
      console.warn('获取歌词信息失败:', error);
      return result;
    }
  }

  // ── 工厂方法 ────────────────────────────────────

  /** 批量创建 */
  static fromDataArray(arr) {
    return (arr || []).map((d) => (d instanceof Song ? d : new Song(d)));
  }

  /** 条件插值: 当字段缺失时从后端 JSON 更新 */
  patch(data) {
    if (data.id !== undefined) this.id = data.id;
    if (data.title !== undefined) this.title = data.title;
    if (data.artist_names !== undefined) this.artistNames = data.artist_names;
    if (data.album_title !== undefined) this.albumTitle = data.album_title;
    if (data.duration !== undefined) this.duration = data.duration;
    if (data.artist_ids !== undefined) this.artistIds = data.artist_ids;
    if (data.album_id !== undefined) this.albumId = data.album_id;
    if (data.lyric_id !== undefined) this.lyricId = data.lyric_id;
    if (data.source_ids !== undefined)
      this.sourceIds = data.source_ids.map((s) =>
        s instanceof SourceId ? s : new SourceId(s),
      );
    return this;
  }
}

/** @deprecated 使用 Song */
export { Song as Track };
