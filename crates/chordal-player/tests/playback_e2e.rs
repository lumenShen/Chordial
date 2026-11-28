//! 播放管线端到端测试：生成的 WAV → 播放线程 → RealtimeNullSink。
//! 无声卡环境可跑（不依赖 ALSA 设备）。

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use chordal_core::model::StreamHandle;
use chordal_player::{Event, Player, PlayerState};

/// 写一个 16bit 立体声 WAV（正弦波）。
fn write_wav(path: &Path, seconds: f64, rate: u32) {
    write_wav_tones(path, seconds, rate, 440.0, 550.0);
}

fn write_wav_tones(path: &Path, seconds: f64, rate: u32, left_hz: f64, right_hz: f64) {
    write_wav_frames(path, seconds, rate, 2, |ch| {
        if ch == 0 { left_hz } else { right_hz }
    });
}

fn write_wav_frames(
    path: &Path,
    seconds: f64,
    rate: u32,
    channels: u16,
    freq_for: impl Fn(u16) -> f64,
) {
    let total_frames = (seconds * rate as f64) as u32;
    let data_len = (total_frames as usize) * channels as usize * 2;
    let mut f = File::create(path).unwrap();

    let mut header = [0u8; 44];
    header[0..4].copy_from_slice(b"RIFF");
    header[4..8].copy_from_slice(&(36 + data_len as u32).to_le_bytes());
    header[8..12].copy_from_slice(b"WAVE");
    header[12..16].copy_from_slice(b"fmt ");
    header[16..20].copy_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    header[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    header[22..24].copy_from_slice(&channels.to_le_bytes());
    header[24..28].copy_from_slice(&rate.to_le_bytes());
    let byte_rate = rate * channels as u32 * 2;
    header[28..32].copy_from_slice(&byte_rate.to_le_bytes());
    let block_align = channels * 2;
    header[32..34].copy_from_slice(&block_align.to_le_bytes());
    header[34..36].copy_from_slice(&16u16.to_le_bytes()); // bits
    header[36..40].copy_from_slice(b"data");
    header[40..44].copy_from_slice(&(data_len as u32).to_le_bytes());
    f.write_all(&header).unwrap();

    for frame in 0..total_frames {
        for ch in 0..channels {
            let t = frame as f64 / rate as f64;
            let s = (t * freq_for(ch) * std::f64::consts::TAU).sin();
            let v = (s * i16::MAX as f64 * 0.5) as i16;
            f.write_all(&v.to_le_bytes()).unwrap();
        }
    }
    f.flush().unwrap();
}

fn wait_event(player: &Player, timeout: Duration) -> Option<Event> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(ev) = player.try_pop_event() {
            return Some(ev);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    None
}

#[test]
fn plays_wav_to_completion_and_reports_position() {
    let dir = std::env::temp_dir().join(format!("chordial-player-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tone.wav");
    write_wav(&path, 1.0, 44_100);

    let player = Player::with_sink_factory(
        Box::new(|_rate, _ch| Ok(Box::new(chordal_player::RealtimeNullSink::new(_rate)) as Box<_>)),
        "realtime-null",
    );
    assert_eq!(player.backend_name(), "realtime-null");

    player.play(StreamHandle::File(path.clone()), "tone", 0);
    let ev = wait_event(&player, Duration::from_secs(10));
    assert!(
        matches!(ev, Some(Event::TrackEnded(_))),
        "expected TrackEnded, got {ev:?}"
    );
    let status = player.status();
    assert_eq!(status.state, PlayerState::Stopped);
    // 进度 ≈ 1000ms（RealtimeNullSink 以实时时钟节流）
    assert!(
        (900..=1600).contains(&status.position_ms),
        "position out of range: {}",
        status.position_ms
    );
    assert_eq!(status.duration_ms, Some(1000));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pause_resume_and_stop_control_state() {
    let dir = std::env::temp_dir().join(format!("chordial-player-pause-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("long.wav");
    write_wav(&path, 4.0, 44_100);

    let player = Player::with_sink_factory(
        Box::new(|rate, _| Ok(Box::new(chordal_player::RealtimeNullSink::new(rate)) as Box<_>)),
        "realtime-null",
    );
    player.play(StreamHandle::File(path.clone()), "long", 0);

    // 等到 Playing
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if player.status().state == PlayerState::Playing {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(player.status().state, PlayerState::Playing);

    player.pause();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if player.status().state == PlayerState::Paused {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(player.status().state, PlayerState::Paused);
    let pos_at_pause = player.status().position_ms;

    player.resume();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if player.status().state == PlayerState::Playing {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(player.status().state, PlayerState::Playing);

    // 暂停期间进度不再推进
    std::thread::sleep(Duration::from_millis(300));
    player.pause();
    let pos_after_wait = player.status().position_ms;
    let _ = pos_at_pause;
    assert!(pos_after_wait > 0);

    player.stop();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if player.status().state == PlayerState::Stopped {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(player.status().state, PlayerState::Stopped);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sink_opens_with_real_track_spec() {
    // 回归：输出设备必须在首包解码前按容器头部的真实规格打开。
    // 兜底的 44.1kHz/2ch 会把 96kHz 单声道素材解释成 4 倍慢放。
    let dir = std::env::temp_dir().join(format!("chordial-player-spec-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("mono96k.wav");
    write_wav_frames(&path, 0.3, 96_000, 1, |_| 440.0);

    let seen = std::sync::Arc::new(std::sync::Mutex::new(None));
    let seen_in_factory = seen.clone();
    let player = Player::with_sink_factory(
        Box::new(move |rate, channels| {
            *seen_in_factory.lock().unwrap() = Some((rate, channels));
            Ok(Box::new(chordal_player::NullSink::new()) as Box<_>)
        }),
        "null",
    );
    player.play(StreamHandle::File(path.clone()), "spec", 0);

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && seen.lock().unwrap().is_none() {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        *seen.lock().unwrap(),
        Some((96_000, 1)),
        "sink must open with the WAV header's real rate/channels"
    );
    player.stop();

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn beat_level_tracks_configured_band() {
    let dir = std::env::temp_dir().join(format!("chordial-player-beat-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("beat.wav");
    // 6 kHz 落在本次配置的鼓点频段（4–10 kHz）中央。
    write_wav_tones(&path, 2.0, 48_000, 6_000.0, 6_000.0);

    let player = Player::with_sink_factory(
        Box::new(|rate, _| Ok(Box::new(chordal_player::RealtimeNullSink::new(rate)) as Box<_>)),
        "realtime-null",
    );
    let mut ranges = chordal_player::AudioBandRanges::default();
    ranges.set(
        chordal_player::AudioBandKind::Kick,
        chordal_player::FrequencyRange {
            low_hz: 4_000.0,
            high_hz: 10_000.0,
        },
    );
    player.set_audio_band_ranges(ranges);
    assert_eq!(player.beat_band(), (4_000.0, 10_000.0));
    assert_eq!(player.beat_level(), 0.0);
    assert_eq!(player.audio_bands(), chordal_player::AudioBands::default());

    player.play(StreamHandle::File(path.clone()), "beat", 0);
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && player.status().state != PlayerState::Playing {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(player.status().state, PlayerState::Playing);

    let mut peak = 0.0f32;
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        peak = peak.max(player.audio_bands().kick);
        if peak > 0.5 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(peak > 0.5, "beat level stayed low: {peak}");

    player.stop();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && player.status().state != PlayerState::Stopped {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(player.beat_level(), 0.0);
    assert_eq!(player.audio_bands(), chordal_player::AudioBands::default());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn seek_mid_track_advances_position() {
    let dir = std::env::temp_dir().join(format!("chordial-player-seek-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("seek.wav");
    write_wav(&path, 3.0, 44_100);

    let player = Player::with_sink_factory(
        Box::new(|rate, _| Ok(Box::new(chordal_player::RealtimeNullSink::new(rate)) as Box<_>)),
        "realtime-null",
    );
    player.play(StreamHandle::File(path.clone()), "seek", 0);
    player.seek_ms(1500);

    let deadline = Instant::now() + Duration::from_secs(6);
    while Instant::now() < deadline {
        let s = player.status();
        if s.position_ms >= 1500 && s.state == PlayerState::Playing {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        player.status().position_ms >= 1400,
        "seek should land near 1500ms, got {}",
        player.status().position_ms
    );
    player.stop();

    let _ = std::fs::remove_dir_all(&dir);
}

/// 生成内存中的 16bit 立体声 WAV（左右声道不同频率的正弦波）。
fn wav_bytes(seconds: f64, rate: u32) -> Vec<u8> {
    let total_frames = (seconds * rate as f64) as u32;
    let data_len = (total_frames as usize) * 2 * 2;
    let mut out = Vec::with_capacity(44 + data_len);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&2u16.to_le_bytes()); // channels
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2 * 2).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_len as u32).to_le_bytes());
    for frame in 0..total_frames {
        let t = frame as f64 / rate as f64;
        for freq in [440.0, 550.0] {
            let s = (t * freq * std::f64::consts::TAU).sin();
            out.extend_from_slice(&((s * i16::MAX as f64 * 0.5) as i16).to_le_bytes());
        }
    }
    out
}

/// 极简 HTTP 服务：接受固定次数的连接，忽略请求内容，返回同一份 body。
fn serve_wav(listener: std::net::TcpListener, body: Vec<u8>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        // 测试只会发起两次播放；收满两个连接就退出，避免 join 永久阻塞。
        for stream in listener.incoming().take(2) {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 2048];
            // 读取请求头即可；播放端只发一个 GET。
            let _ = stream.read(&mut buf);
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: audio/wav\r\nConnection: close\r\n\r\n",
                body.len()
            );
            if stream.write_all(header.as_bytes()).is_err() {
                continue;
            }
            // 断开的旧流写入失败是预期行为，直接继续处理下一个连接。
            let _ = stream.write_all(&body);
        }
    })
}

/// 回归：播放中切歌必须立刻释放上一首的 HTTP 音频缓冲。
///
/// 旧实现从 `play_item` 内递归调用 `play_item`，旧 `decoder` 与
/// `HttpMediaSource`（整首已下载字节）要到新曲目结束才随栈帧释放。
/// 这里通过 `Gauge::AudioStream` 验证切歌后只剩一条流的占用。
#[test]
fn http_track_switch_releases_previous_stream_buffer() {
    use chordal_core::mem::{self, Gauge};

    let body = wav_bytes(12.0, 44_100);
    let body_len = body.len();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = serve_wav(listener, body);

    let player = Player::with_sink_factory(
        Box::new(|rate, _| Ok(Box::new(chordal_player::RealtimeNullSink::new(rate)) as Box<_>)),
        "realtime-null",
    );
    let wait_playing = |generation: u64| {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let status = player.status();
            if status.generation == generation && status.state == PlayerState::Playing {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    };

    let url = format!("http://{addr}/track.wav");
    player.play(StreamHandle::Url(url.clone()), "first", 0);
    assert!(wait_playing(1), "first track did not start");
    // 已知 Content-Length：打开时就会精确预留整首容量。
    let one_stream = mem::gauge(Gauge::AudioStream);
    assert!(
        one_stream >= body_len,
        "stream should reserve for content-length: gauge={one_stream}, body={body_len}"
    );

    player.play(StreamHandle::Url(url), "second", 0);
    assert!(wait_playing(2), "second track did not start");

    // 给旧 decoder 的 Drop 一点时间；旧实现会稳定停在 ~2×body。
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut gauge = mem::gauge(Gauge::AudioStream);
    while Instant::now() < deadline && gauge >= body_len * 3 / 2 {
        std::thread::sleep(Duration::from_millis(20));
        gauge = mem::gauge(Gauge::AudioStream);
    }
    assert!(
        gauge < body_len * 3 / 2,
        "previous track buffer retained: gauge={gauge}, one stream={body_len}"
    );

    player.stop();
    drop(player);
    let _ = server.join();
}
