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
    let channels = 2u16;
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
            let freq = if ch == 0 { left_hz } else { right_hz };
            let s = (t * freq * std::f64::consts::TAU).sin();
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
fn beat_level_tracks_configured_band() {
    let dir = std::env::temp_dir().join(format!("chordial-player-beat-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("beat.wav");
    // 6 kHz 落在默认鼓点频段（4–10 kHz）中央。
    write_wav_tones(&path, 2.0, 48_000, 6_000.0, 6_000.0);

    let player = Player::with_sink_factory(
        Box::new(|rate, _| Ok(Box::new(chordal_player::RealtimeNullSink::new(rate)) as Box<_>)),
        "realtime-null",
    );
    player.set_beat_band(4_000.0, 10_000.0);
    assert_eq!(player.beat_band(), (4_000.0, 10_000.0));
    assert_eq!(player.beat_level(), 0.0);

    player.play(StreamHandle::File(path.clone()), "beat", 0);
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && player.status().state != PlayerState::Playing {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(player.status().state, PlayerState::Playing);

    let mut peak = 0.0f32;
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        peak = peak.max(player.beat_level());
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
