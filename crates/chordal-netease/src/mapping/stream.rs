//! 网易云映射：stream（由 mapping.rs 拆分，行为不变）。

use super::*;

#[derive(Deserialize)]
pub(super) struct SongUrlResp {
    #[serde(default)]
    data: Option<Vec<SongUrlData>>,
}

#[derive(Deserialize)]
pub(super) struct SongUrlData {
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    url: Option<String>,
    #[serde(rename = "type", default)]
    format: Option<String>,
    #[serde(default)]
    br: Option<u64>,
    #[serde(default)]
    level: Option<String>,
    #[serde(default, rename = "freeTrialInfo")]
    free_trial_info: Option<serde_json::Value>,
}

/// `/song/url/v1` 响应 → 播放句柄 + 实际音质。`url` 为空 = VIP/无版权。
pub fn map_song_url(body: &serde_json::Value, want_id: &str) -> CoreResult<ResolvedStream> {
    let resp: SongUrlResp = serde_json::from_value(body.clone())
        .map_err(|e| CoreError::source_err("netease", e.to_string()))?;
    let data = resp.data.unwrap_or_default();
    let entry = data
        .iter()
        .find(|d| d.id.map(|i| i.to_string()) == Some(want_id.to_string()))
        .or_else(|| data.first());

    let Some(entry) = entry else {
        return Err(CoreError::NoStream {
            track: format!("netease:{want_id}"),
            reason: "empty data".into(),
        });
    };
    let Some(url) = entry.url.as_deref().filter(|u| !u.is_empty()) else {
        return Err(CoreError::NoStream {
            track: format!("netease:{want_id}"),
            reason: "no url (vip / region-locked / removed)".into(),
        });
    };
    if let Some(info) = &entry.free_trial_info {
        // 未登录 / 非会员时网易云返回 30-45s 试听片段；仍可播放，但 UI 应提示。
        tracing::warn!(
            target: "chordial::netease",
            track = want_id,
            trial = %info,
            "netease returned a trial fragment (login/VIP required for full track)"
        );
    }
    let codec = entry
        .format
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("mp3")
        .to_ascii_lowercase();
    let bitrate_kbps = entry.br.and_then(|br| {
        let kbps = (br / 1000) as u32;
        (kbps > 0).then_some(kbps)
    });
    let lossless = codec.contains("flac") || codec.contains("alac");
    let hires = entry
        .level
        .as_deref()
        .is_some_and(|l| l.eq_ignore_ascii_case("hires"));
    let tier = if hires {
        QualityTier::HiRes
    } else if lossless {
        QualityTier::Lossless
    } else {
        match bitrate_kbps {
            Some(kbps) if kbps >= 256 => QualityTier::High,
            Some(_) => QualityTier::Standard,
            None => QualityTier::Unknown,
        }
    };
    let quality = AudioQuality {
        tier,
        codec: Some(codec),
        bitrate_kbps,
        sample_rate_hz: None,
        lossless,
    };
    Ok(ResolvedStream {
        handle: StreamHandle::Url(url.to_string()),
        quality: Some(quality),
    })
}

// ---------- /lyric ----------
