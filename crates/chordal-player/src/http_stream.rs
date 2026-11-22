//! HTTP 渐进式流 → symphonia `MediaSource`。
//!
//! 模型：后台下载线程（reqwest blocking，播放线程专用，不占 tokio runtime）
//! 把字节持续写入共享缓冲；解码侧通过 `Read` 拉取，未下载到的位置用
//! Condvar 等待 —— 播放开始的延迟只有一个前置缓冲窗，不必等整首歌。
//!
//! 内存边界：**全量缓存已下载字节**（典型曲目 ≤ 50MB）。换来的是任意
//! 向后 seek 都是 O(1) 内存移动、向前的 seek 只是等网络，实现简单且
//! 无 O(n²) 重传。若未来需要超长流再换滑窗实现。

use std::io::{Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use chordal_core::error::{CoreError, CoreResult};
use symphonia::core::io::MediaSource;

const CHUNK: usize = 32 * 1024;

struct Shared {
    data: Vec<u8>,
    done: bool,
    error: Option<String>,
    content_length: Option<u64>,
}

struct StreamShared {
    state: Mutex<Shared>,
    cond: Condvar,
    cancel: AtomicBool,
}

/// 可 seek 的 HTTP 流（symphonia MediaSource）。
pub struct HttpMediaSource {
    shared: Arc<StreamShared>,
    pos: usize,
}

impl HttpMediaSource {
    /// 发起 GET（支持 Range 重定向到 0 的场景由服务端处理）并启动后台下载。
    pub fn open(url: &str) -> CoreResult<Self> {
        let shared = Arc::new(StreamShared {
            state: Mutex::new(Shared {
                data: Vec::with_capacity(256 * 1024),
                done: false,
                error: None,
                content_length: None,
            }),
            cond: Condvar::new(),
            cancel: AtomicBool::new(false),
        });

        let worker_shared = shared.clone();
        let url = url.to_string();
        // 播放线程专用的阻塞下载线程：reqwest::blocking 自带独立 runtime，
        // 与 UI 的 executor / 共享 tokio runtime 互不干扰。
        std::thread::Builder::new()
            .name("chordial-http-stream".into())
            .spawn(move || download_worker(&url, worker_shared))
            .map_err(|e| CoreError::Other(format!("spawn download thread: {e}")))?;

        Ok(Self { shared, pos: 0 })
    }
}

impl Drop for HttpMediaSource {
    fn drop(&mut self) {
        self.shared.cancel.store(true, Ordering::Release);
        self.shared.cond.notify_all();
    }
}

fn download_worker(url: &str, shared: Arc<StreamShared>) {
    let client = match reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(8))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            finish(&shared, Err(format!("client: {e}")));
            return;
        }
    };
    let resp = match client.get(url).send() {
        Ok(r) => r,
        Err(e) => {
            finish(&shared, Err(format!("GET {url}: {e}")));
            return;
        }
    };
    if !resp.status().is_success() {
        finish(&shared, Err(format!("HTTP {}", resp.status())));
        return;
    }
    let content_length = resp.content_length();
    {
        let mut state = shared.state.lock().expect("stream lock poisoned");
        state.content_length = content_length;
    }
    shared.cond.notify_all();

    let mut resp = resp;
    let mut buf = vec![0u8; CHUNK];
    loop {
        if shared.cancel.load(Ordering::Acquire) {
            finish(&shared, Ok(())); // 取消：不再写数据，静默退出
            return;
        }
        match resp.read(&mut buf) {
            Ok(0) => {
                finish(&shared, Ok(()));
                return;
            }
            Ok(n) => {
                let mut state = shared.state.lock().expect("stream lock poisoned");
                state.data.extend_from_slice(&buf[..n]);
                shared.cond.notify_all();
            }
            Err(e) => {
                finish(&shared, Err(format!("read: {e}")));
                return;
            }
        }
    }
}

fn finish(shared: &StreamShared, result: Result<(), String>) {
    let mut state = shared.state.lock().expect("stream lock poisoned");
    state.done = true;
    if let Err(msg) = result {
        state.error = Some(msg);
    }
    shared.cond.notify_all();
}

impl Read for HttpMediaSource {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut state = self.shared.state.lock().expect("stream lock poisoned");
        // 等待：目标区间已下载 或 全部结束。
        while self.pos >= state.data.len() && !state.done {
            if self.shared.cancel.load(Ordering::Acquire) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "stream cancelled",
                ));
            }
            state = self.shared.cond.wait(state).expect("stream lock poisoned");
        }
        if let Some(err) = &state.error
            && self.pos >= state.data.len()
        {
            return Err(std::io::Error::other(err.clone()));
        }
        if self.pos >= state.data.len() {
            return Ok(0); // EOF
        }
        let available = state.data.len() - self.pos;
        let n = buf.len().min(available);
        buf[..n].copy_from_slice(&state.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

impl Seek for HttpMediaSource {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let new_pos = match pos {
            SeekFrom::Start(n) => n as i64,
            SeekFrom::Current(d) => self.pos as i64 + d,
            SeekFrom::End(d) => {
                let state = self.shared.state.lock().expect("stream lock poisoned");
                let Some(total) = state.content_length else {
                    return Err(std::io::Error::other(
                        "seek from end unavailable (no content-length)",
                    ));
                };
                total as i64 + d
            }
        };
        if new_pos < 0 {
            return Err(std::io::Error::other("seek before start"));
        }
        self.pos = new_pos as usize;
        Ok(self.pos as u64)
    }
}

impl MediaSource for HttpMediaSource {
    fn is_seekable(&self) -> bool {
        true
    }

    /// 已知 content-length 时返回总长（下载中也可给——长度不随进度变化）。
    fn byte_len(&self) -> Option<u64> {
        let state = self.shared.state.lock().expect("stream lock poisoned");
        state.content_length
    }
}
