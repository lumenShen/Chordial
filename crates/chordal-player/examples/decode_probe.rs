//! 用真实 HTTP 直链验证“边下边播 + symphonia 解码”链路。
//!
//! ```bash
//! cargo run -p chordal-player --example decode_probe -- "https://m801.music.126.net/....mp3"
//! ```

use chordal_player::decode::AudioDecoder;
use chordal_player::http_stream::HttpMediaSource;

fn main() {
    let url = match std::env::args().nth(1) {
        Some(url) if url.starts_with("http") => url,
        _ => {
            eprintln!("usage: decode_probe <http(s) audio url>");
            return;
        }
    };
    let source = match HttpMediaSource::open(&url) {
        Ok(source) => source,
        Err(err) => {
            eprintln!("open stream failed: {err}");
            return;
        }
    };
    let mut decoder = match AudioDecoder::open_media(Box::new(source), "mp3") {
        Ok(decoder) => decoder,
        Err(err) => {
            eprintln!("open decoder failed: {err}");
            return;
        }
    };
    println!(
        "decoder: {} Hz, {} ch, duration={:?}ms",
        decoder.sample_rate(),
        decoder.channels(),
        decoder.duration_ms()
    );
    let mut samples = Vec::with_capacity(16_384);
    let mut total_frames = 0u64;
    for block in 0..80 {
        match decoder.next_block(&mut samples) {
            Ok(true) => {
                total_frames += (samples.len() / decoder.channels()) as u64;
                if block == 0 {
                    let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
                    println!(
                        "first block: {} samples, peak={:.4}, first={:?}",
                        samples.len(),
                        peak,
                        &samples[..samples.len().min(6)]
                    );
                }
            }
            Ok(false) => break,
            Err(err) => {
                eprintln!("decode failed at block {block}: {err}");
                return;
            }
        }
    }
    println!(
        "decoded {total_frames} frames ({:.2}s)",
        total_frames as f64 / decoder.sample_rate() as f64
    );
}
