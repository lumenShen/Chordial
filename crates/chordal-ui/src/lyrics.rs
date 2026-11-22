//! 领域歌词 → AMLL 歌词模型转换，以及专辑封面下载/解码。
//!
//! 所有耗时操作（网络、图片解码）都在 gpui 的 background executor 上执行，
//! UI 线程只接收已经解码好的 RGBA 像素。

use std::sync::Arc;

use chordal_cache::{CacheTags, ObjectCache, now_secs, object_key};
use chordal_core::model::{Lyrics as CoreLyrics, SourceId, Track};
use chordal_core::perf::PerfScope;
use chordial_amll_gpui::model::{LyricLine, LyricWord};

/// 把核心库的逐行歌词转换成 AMLL 行模型。
///
/// - 行结束时间取下一行开始时间；最后一行取 `duration_ms`（未知时 +8s）；
/// - 译文按行下标并入 `translated_lyric`；
/// - 单行内只有一个词（LRC 语义），逐词格式由其它解析器产生。
pub fn to_amll_lyrics(lyrics: &CoreLyrics, duration_ms: Option<u64>) -> Vec<LyricLine> {
    let scope = PerfScope::enter("amll.convert");
    let lines = if !lyrics.timed_lines.is_empty() {
        to_amll_timed(lyrics, duration_ms)
    } else {
        to_amll_plain(lyrics, duration_ms)
    };
    scope.finish(lines.len() as u64);
    lines
}

fn to_amll_plain(lyrics: &CoreLyrics, duration_ms: Option<u64>) -> Vec<LyricLine> {
    let n = lyrics.lines.len();
    let mut out = Vec::with_capacity(n);
    for (i, line) in lyrics.lines.iter().enumerate() {
        let start = line.time_ms;
        let end = if i + 1 < n {
            let next = lyrics.lines[i + 1].time_ms;
            if next > start { next } else { start + 1_000 }
        } else {
            let fallback = start + 8_000;
            duration_ms
                .map(|d| d.clamp(start + 1_000, fallback))
                .unwrap_or(fallback)
        };
        let text = line.text.trim_end().to_string();
        // 纯时间戳的空行（常见于 LRC 的间奏占位）不生成可见行，否则会出现
        // 空白高亮行与“只有时间戳”的假歌词。
        if text.is_empty()
            && lyrics
                .translation
                .get(i)
                .is_none_or(|t| t.trim().is_empty())
        {
            continue;
        }
        let words = if text.is_empty() {
            Vec::new()
        } else {
            vec![LyricWord::new(text, start, end)]
        };
        out.push(LyricLine {
            words,
            translated_lyric: lyrics
                .translation
                .get(i)
                .map(|t| t.trim_end().to_string())
                .unwrap_or_default(),
            roman_lyric: String::new(),
            start_time: start,
            end_time: end,
            is_bg: false,
            is_duet: false,
        });
    }
    out
}

/// YRC/逐词歌词 → AMLL 模型。保留词级时间，渲染层会自动进入整行扫光模式。
///
/// 以 LRC 行级歌词为骨架（避免 YRC 缺失/多出的行被丢掉），按最近时间戳
/// 挂接逐词时间；YRC 独有的行再追加。整个合并 O((n+m) log m)。
fn to_amll_timed(lyrics: &CoreLyrics, duration_ms: Option<u64>) -> Vec<LyricLine> {
    let timed = &lyrics.timed_lines;
    let mut used = vec![false; timed.len()];
    let mut out = Vec::with_capacity(lyrics.lines.len() + timed.len());

    for (i, line) in lyrics.lines.iter().enumerate() {
        let start = line.time_ms;
        let pos = timed.partition_point(|t| t.start_ms < start);
        let mut matched = None;
        let mut best_delta = 251u64;
        for candidate in [pos.saturating_sub(1), pos, pos + 1] {
            if let Some(t) = timed.get(candidate)
                && !used[candidate]
            {
                let delta = t.start_ms.abs_diff(start);
                if delta < best_delta {
                    best_delta = delta;
                    matched = Some(candidate);
                }
            }
        }

        let next_start = lyrics.lines.get(i + 1).map(|l| l.time_ms);
        let mut end = match next_start {
            Some(next) if next > start => next,
            _ => duration_ms
                .map(|d| d.max(start + 1_000))
                .unwrap_or(start + 8_000),
        };

        let words = if let Some(j) = matched {
            used[j] = true;
            // 非末行用 YRC 自带行尾（避免相邻行同时高亮）；末行延到曲目结束。
            end = if next_start.is_some() {
                timed[j].end_ms.max(start + 1)
            } else {
                end.max(timed[j].end_ms).max(start + 1)
            };
            timed[j]
                .words
                .iter()
                .map(|w| LyricWord {
                    start_time: w.start_ms,
                    end_time: w.end_ms.max(w.start_ms + 1),
                    word: w.text.clone(),
                    roman_word: String::new(),
                    obscene: false,
                })
                .collect::<Vec<_>>()
        } else {
            let text = line.text.trim_end().to_string();
            if text.is_empty() {
                vec![]
            } else {
                vec![LyricWord::new(text, start, end.max(start + 1))]
            }
        };

        let translated = lyrics
            .translation
            .get(i)
            .map(|t| t.trim_end().to_string())
            .unwrap_or_default();
        if words.is_empty() && translated.is_empty() {
            continue;
        }
        out.push(LyricLine {
            words,
            translated_lyric: translated,
            roman_lyric: String::new(),
            start_time: start,
            end_time: end,
            is_bg: false,
            is_duet: false,
        });
    }

    // YRC 独有的行（LRC 缺失）追加进去。
    for (j, timed_line) in timed.iter().enumerate() {
        if used[j] || timed_line.words.iter().all(|w| w.text.trim().is_empty()) {
            continue;
        }
        let start = timed_line.start_ms;
        let end = match timed.get(j + 1) {
            Some(_) => timed_line.end_ms.max(start + 1),
            None => duration_ms
                .map(|d| d.max(start + 1_000))
                .unwrap_or(timed_line.end_ms.max(start + 1)),
        };
        out.push(LyricLine {
            words: timed_line
                .words
                .iter()
                .map(|w| LyricWord {
                    start_time: w.start_ms,
                    end_time: w.end_ms.max(w.start_ms + 1),
                    word: w.text.clone(),
                    roman_word: String::new(),
                    obscene: false,
                })
                .collect(),
            translated_lyric: translation_for_start(lyrics, start),
            roman_lyric: String::new(),
            start_time: start,
            end_time: end.max(timed_line.end_ms),
            is_bg: false,
            is_duet: false,
        });
    }

    out.sort_by_key(|l| l.start_time);
    out
}

fn translation_for_start(lyrics: &CoreLyrics, start: u64) -> String {
    let idx = lyrics.lines.partition_point(|l| l.time_ms < start);
    // YRC 与 LRC 的时间戳可能相差几十毫秒，按最近行容错匹配。
    for candidate in [idx.checked_sub(1), Some(idx)].into_iter().flatten() {
        if let (Some(line), Some(trans)) = (
            lyrics.lines.get(candidate),
            lyrics.translation.get(candidate),
        ) && line.time_ms.abs_diff(start) <= 120
        {
            return trans.trim_end().to_string();
        }
    }
    String::new()
}

/// 从曲目元数据里取封面地址。
pub fn cover_url(track: &Track) -> Option<String> {
    track
        .album
        .as_ref()
        .and_then(|album| album.cover.clone())
        .filter(|url| !url.trim().is_empty())
}

static COVER_CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();

fn cover_client() -> Option<&'static reqwest::Client> {
    Some(COVER_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(8))
            .build()
            .expect("build reqwest client")
    }))
}

const MAX_COVER_BYTES: u64 = 24 * 1024 * 1024;
const MAX_COVER_DIMENSION: u32 = 8192;

/// 网易云 CDN 支持 `?param=WxH` 服务端缩放；用它避免下载/解码 6000×6000 原图。
fn cdn_resize(url: &str) -> String {
    let is_netease = url.contains("126.net") || url.contains("127.net");
    if is_netease && !url.contains("param=") {
        let sep = if url.contains('?') { '&' } else { '?' };
        return format!("{url}{sep}param=512y512");
    }
    url.to_string()
}

/// 下载并解码封面为 RGBA8（最长边不超过 512，内存上界约 1MB）。
pub async fn fetch_cover_rgba(url: String) -> Option<(u32, u32, Vec<u8>)> {
    let (bytes, _) = fetch_cover_bytes(&url).await?;
    decode_cover_bytes(&bytes)
}

/// 只取原始字节（网络/本地文件）+ 可用于条件刷新的响应标签。
async fn fetch_cover_bytes(url: &str) -> Option<(Vec<u8>, CacheTags)> {
    let url = cdn_resize(url);
    if let Some(path) = url.strip_prefix("file://") {
        // 本地同名封面：已在 background executor 上，直接读小图。
        let meta = std::fs::metadata(path).ok()?;
        if meta.len() > MAX_COVER_BYTES {
            return None;
        }
        let bytes = std::fs::read(path).ok()?;
        return (bytes.len() as u64 <= MAX_COVER_BYTES).then_some((bytes, CacheTags::default()));
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        let response = cover_client()?
            .get(url)
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?;
        let tags = CacheTags {
            etag: header_string(&response, reqwest::header::ETAG),
            last_modified: header_string(&response, reqwest::header::LAST_MODIFIED),
            content_type: header_string(&response, reqwest::header::CONTENT_TYPE),
            expires_at: None,
        };
        let bytes = response.bytes().await.ok()?.to_vec();
        return (bytes.len() as u64 <= MAX_COVER_BYTES).then_some((bytes, tags));
    }
    None
}

/// 解码 + 限制尺寸。CPU 密集段，调用方应在后台线程执行。
pub fn decode_cover_bytes(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    if bytes.len() as u64 > MAX_COVER_BYTES {
        return None;
    }
    // 先只读尺寸，再带硬限制解码，拒绝解压炸弹。
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let (w0, h0) = reader.into_dimensions().ok()?;
    if w0 == 0 || h0 == 0 || w0 > MAX_COVER_DIMENSION || h0 > MAX_COVER_DIMENSION {
        return None;
    }
    let scope = PerfScope::enter("amll.cover.decode");
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_COVER_DIMENSION);
    limits.max_image_height = Some(MAX_COVER_DIMENSION);
    limits.max_alloc = Some(320 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().ok()?.into_rgba8();
    scope.finish(bytes.len() as u64);
    let (w, h) = image.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    let (w, h, image) = if w > 512 || h > 512 {
        let scale = 512.0 / w.max(h) as f32;
        let nw = ((w as f32 * scale).round() as u32).max(1);
        let nh = ((h as f32 * scale).round() as u32).max(1);
        (
            nw,
            nh,
            image::imageops::resize(&image, nw, nh, image::imageops::FilterType::Triangle),
        )
    } else {
        (w, h, image)
    };
    Some((w, h, image.into_raw()))
}

/// 解码 `data:image/png;base64,...`（登录二维码）。
pub fn decode_data_url_image(data_url: &str) -> Option<(u32, u32, Vec<u8>)> {
    use base64::Engine as _;
    let payload = data_url
        .split_once(',')
        .map(|(_, payload)| payload)
        .unwrap_or(data_url);
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload.trim())
        .ok()?;
    decode_cover_bytes(&bytes)
}

/// 封面缓存刷新阈值：网易 CDN 的专辑封面基本不变，7 天检查一次即可。
const COVER_REFRESH_AFTER_SECS: u64 = 7 * 24 * 60 * 60;

/// 先读本地对象缓存，命中后后台做一次条件刷新（ETag/Last-Modified/内容摘要）。
/// 未命中则同步抓取并写入缓存。
async fn fetch_cover_cached(
    cache: Option<Arc<ObjectCache>>,
    namespace: String,
    url: String,
) -> Option<(u32, u32, Vec<u8>)> {
    let resized = cdn_resize(&url);
    if resized.starts_with("file://") || cache.is_none() {
        return fetch_cover_rgba(url).await;
    }
    let cache = cache.expect("checked");
    let key = object_key(&namespace, "cover", &resized);

    if let Some(meta) = cache.meta(&key)
        && let Some(obj) = cache.get(&key)
        && let Some(decoded) = decode_cover_bytes(&obj.bytes)
    {
        if now_secs().saturating_sub(meta.fetched_at) >= COVER_REFRESH_AFTER_SECS {
            spawn_cover_refresh(
                cache.clone(),
                key,
                resized.clone(),
                meta.etag,
                meta.last_modified,
            );
        }
        return Some(decoded);
    }

    let (bytes, tags) = fetch_cover_bytes(&resized).await?;
    cache.put(&key, &bytes, tags);
    let decoded = decode_cover_bytes(&bytes);
    if decoded.is_none() {
        cache.remove(&key);
    }
    decoded
}

fn spawn_cover_refresh(
    cache: Arc<ObjectCache>,
    key: String,
    url: String,
    etag: Option<String>,
    last_modified: Option<String>,
) {
    chordal_netease::shared_runtime().spawn(async move {
        let Some(client) = cover_client() else {
            return;
        };
        let mut request = client.get(&url);
        if let Some(etag) = &etag {
            request = request.header(reqwest::header::IF_NONE_MATCH, etag);
        }
        if let Some(last_modified) = &last_modified {
            request = request.header(reqwest::header::IF_MODIFIED_SINCE, last_modified);
        }
        let Ok(response) = request.send().await else {
            return;
        };
        if response.status() == reqwest::StatusCode::NOT_MODIFIED {
            cache.touch(
                &key,
                CacheTags {
                    etag,
                    last_modified,
                    ..Default::default()
                },
            );
            return;
        }
        if !response.status().is_success() {
            return;
        }
        let etag = header_string(&response, reqwest::header::ETAG);
        let last_modified = header_string(&response, reqwest::header::LAST_MODIFIED);
        let Ok(bytes) = response.bytes().await else {
            return;
        };
        if bytes.len() as u64 > MAX_COVER_BYTES {
            return;
        }
        cache.put(
            &key,
            &bytes,
            CacheTags {
                etag,
                last_modified,
                ..Default::default()
            },
        );
    });
}

fn header_string(
    response: &reqwest::Response,
    name: reqwest::header::HeaderName,
) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

/// 在阻塞线程池里运行封面加载（内部复用 netease 的共享 tokio runtime）。
/// `namespace` 通常是曲目所属源 id；源卸载时缓存按该 namespace 回收。
pub fn fetch_cover_blocking(
    cache: Option<Arc<ObjectCache>>,
    namespace: &SourceId,
    url: String,
) -> Option<(u32, u32, Vec<u8>)> {
    chordal_netease::shared_runtime().block_on(fetch_cover_cached(
        cache,
        namespace.as_str().to_string(),
        url,
    ))
}

/// 只取封面原始字节（用于下载时嵌入音频标签 / 写边车封面）。
/// 与 UI 展示路径一致，网易 CDN 会先缩放到 512×512，避免嵌入原图。
pub fn fetch_cover_raw_blocking(url: String) -> Option<Vec<u8>> {
    chordal_netease::shared_runtime()
        .block_on(async move { fetch_cover_bytes(&url).await.map(|(bytes, _)| bytes) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chordal_core::model::{LyricLine as CoreLine, SourceId, TrackId, TrackOrigin};

    #[test]
    fn converts_lines_and_translation() {
        let lyrics = CoreLyrics {
            lines: vec![
                CoreLine {
                    time_ms: 1_000,
                    text: "hello".into(),
                },
                CoreLine {
                    time_ms: 3_000,
                    text: "world".into(),
                },
            ],
            raw: String::new(),
            translation: vec!["你好".into()],
            timed_lines: Vec::new(),
        };
        let lines = to_amll_lyrics(&lyrics, Some(10_000));
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].start_time, 1_000);
        assert_eq!(lines[0].end_time, 3_000);
        assert_eq!(lines[0].translated_lyric, "你好");
        assert_eq!(lines[1].end_time, 10_000); // 最后一行延到曲目结束
    }

    #[test]
    fn converts_timed_lines_with_translation() {
        use chordal_core::model::{TimedLine, TimedWord};
        let lyrics = CoreLyrics {
            lines: vec![CoreLine {
                time_ms: 1_000,
                text: "你好".into(),
            }],
            raw: String::new(),
            translation: vec!["hello".into()],
            timed_lines: vec![TimedLine {
                start_ms: 1_000,
                end_ms: 2_000,
                words: vec![
                    TimedWord {
                        start_ms: 1_000,
                        end_ms: 1_400,
                        text: "你".into(),
                    },
                    TimedWord {
                        start_ms: 1_400,
                        end_ms: 2_000,
                        text: "好".into(),
                    },
                ],
            }],
        };
        let lines = to_amll_lyrics(&lyrics, Some(10_000));
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].words.len(), 2);
        assert_eq!(lines[0].words[0].word, "你");
        assert_eq!(lines[0].translated_lyric, "hello");
        assert_eq!(lines[0].end_time, 10_000);
    }

    #[test]
    fn filters_timestamp_only_lines() {
        let lyrics = CoreLyrics {
            lines: vec![
                CoreLine {
                    time_ms: 1_000,
                    text: "a".into(),
                },
                CoreLine {
                    time_ms: 2_000,
                    text: "".into(),
                },
            ],
            raw: String::new(),
            translation: Vec::new(),
            timed_lines: Vec::new(),
        };
        let lines = to_amll_lyrics(&lyrics, None);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text(), "a");
    }

    #[test]
    fn cdn_resize_appends_param_once() {
        let url = "https://p4.music.126.net/a/b.jpg";
        let once = cdn_resize(url);
        assert!(once.ends_with("?param=512y512"));
        assert_eq!(cdn_resize(&once), once);
        assert_eq!(
            cdn_resize("https://example.com/c.jpg"),
            "https://example.com/c.jpg"
        );
    }

    #[test]
    #[ignore = "requires network / netease CDN"]
    fn fetch_real_cover_is_bounded() {
        let url = std::env::var("NETEASE_COVER_URL").unwrap_or_else(|_| {
            "https://p4.music.126.net/HXfXIIBCk5w96MiY1Wdsqw==/109951171836610847.jpg".into()
        });
        let (w, h, data) =
            fetch_cover_blocking(None, &SourceId::new("netease"), url).expect("cover");
        assert!(w <= 512 && h <= 512, "{w}x{h}");
        assert_eq!(data.len(), (w * h * 4) as usize);
    }

    #[test]
    fn cover_url_uses_album() {
        let mut track = Track::new(
            TrackId::new(SourceId::new("x"), "1"),
            "t",
            vec![],
            None,
            None,
            TrackOrigin::Remote {
                source: SourceId::new("x"),
            },
        );
        assert!(cover_url(&track).is_none());
        track.album = Some(chordal_core::model::AlbumRef {
            id: None,
            name: "a".into(),
            cover: Some("https://example.com/c.jpg".into()),
        });
        assert_eq!(
            cover_url(&track).as_deref(),
            Some("https://example.com/c.jpg")
        );
    }
}
