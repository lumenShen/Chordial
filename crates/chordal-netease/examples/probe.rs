//! 对运行中的 api-enhanced 做一次端到端探针。
//!
//! ```bash
//! cargo run -p chordal-netease --example probe -- "晴天 周杰伦"
//! NETEASE_COOKIE="MUSIC_U=..." cargo run -p chordal-netease --example probe -- "晴天"
//! ```
//!
//! 会依次验证：健康检查 → 搜索 → song/detail → lyric（含 YRC）→ song/url。
//! 未登录时 song/url 返回空直链是 api-enhanced 的正常行为，探针会打印原因。

use chordal_core::model::{SearchQuery, SourceId};
use chordal_core::source::MusicSource;
use chordal_netease::{NeteaseConfig, NeteaseMode, NeteaseSource, shared_runtime};

fn main() {
    let keywords = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "晴天 周杰伦".to_string());
    let cookie = std::env::var("NETEASE_COOKIE").ok();
    let config = NeteaseConfig {
        base_url: std::env::var("NETEASE_API_BASE")
            .unwrap_or_else(|_| "http://127.0.0.1:3000".to_string()),
        cookie: cookie.clone(),
        level: None,
        mode: NeteaseMode::External,
        ..Default::default()
    };
    let source = NeteaseSource::new(SourceId::new("netease"), config, "网易云");

    shared_runtime().block_on(async move {
        println!("health: {}", source.health().await);
        let results = match source.search(&SearchQuery::track(&keywords)).await {
            Ok(r) => r,
            Err(err) => {
                eprintln!("search failed: {err}");
                return;
            }
        };
        println!("search '{}': {} tracks", keywords, results.track_count());
        for track in results.tracks.iter().take(5) {
            println!(
                "  {} | {} | {} | {}ms",
                track.id.id,
                track.title,
                track
                    .artists
                    .iter()
                    .map(|a| a.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                track.duration_ms.unwrap_or(0)
            );
        }
        let Some(first) = results.tracks.first().cloned() else {
            println!("no track");
            return;
        };

        match source.track(&first.id).await {
            Ok(detail) => println!(
                "detail: {} | album={:?} | cover={:?} | duration={:?}ms",
                detail.title,
                detail.album.as_ref().map(|a| a.name.clone()),
                detail.album.as_ref().and_then(|a| a.cover.clone()),
                detail.duration_ms
            ),
            Err(err) => println!("detail failed: {err}"),
        }

        match source.lyrics(&first.id).await {
            Ok(Some(lyrics)) => {
                let pages: usize = lyrics.translation.iter().map(|t| t.len()).sum::<usize>()
                    / 1400
                    + 1;
                println!(
                    "lyrics: {} lines, {} timed lines, {} translation lines (~{} pages of 1400 chars)",
                    lyrics.lines.len(),
                    lyrics.timed_lines.len(),
                    lyrics.translation.len(),
                    pages
                );
                for line in lyrics.lines.iter().take(5) {
                    println!("  [{}ms] {}", line.time_ms, line.text);
                }
            }
            Ok(None) => println!("lyrics: none"),
            Err(err) => println!("lyrics failed: {err}"),
        }

        match source.stream(&first.id).await {
            Ok(handle) => println!("stream: ok -> {handle:?}"),
            Err(err) => println!("stream: {err}"),
        }
    });
}
