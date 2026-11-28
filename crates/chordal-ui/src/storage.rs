//! UI 持久化 writer：串行、去抖、原子写入。
//!
//! UI 只提交不可变快照；JSON 序列化、磁盘写入和退出 flush 都在独立线程完成。
//! 同一路径的连续 Save 只保留最后一份，避免拖拽排序时反复写完整歌单。

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use serde::Serialize;

const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(300);

type Encode = Box<dyn FnOnce() -> io::Result<Vec<u8>> + Send>;

enum Command {
    Save { path: PathBuf, encode: Encode },
    Flush(mpsc::Sender<io::Result<()>>),
}

pub struct JsonWriter {
    tx: Sender<Command>,
    debounce: Duration,
}

impl fmt::Debug for JsonWriter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JsonWriter")
            .field("debounce", &self.debounce)
            .finish_non_exhaustive()
    }
}

impl JsonWriter {
    pub fn new() -> Arc<Self> {
        Self::with_debounce(DEFAULT_DEBOUNCE)
    }

    pub fn with_debounce(debounce: Duration) -> Arc<Self> {
        let (tx, rx) = mpsc::channel();
        let worker_debounce = debounce;
        thread::Builder::new()
            .name("chordial-json-writer".into())
            .spawn(move || {
                chordal_core::runtime::lower_current_thread_priority();
                writer_loop(rx, worker_debounce);
            })
            .expect("spawn json writer");
        Arc::new(Self { tx, debounce })
    }

    pub fn save_json<T>(&self, path: impl Into<PathBuf>, value: T)
    where
        T: Serialize + Send + 'static,
    {
        let path = path.into();
        let encode: Encode = Box::new(move || serde_json::to_vec(&value).map_err(io::Error::other));
        let _ = self.tx.send(Command::Save { path, encode });
    }

    /// 同步屏障：写入此命令之前收到的全部最新快照，然后返回。
    pub fn flush(&self) -> io::Result<()> {
        let (tx, rx) = mpsc::channel();
        self.tx
            .send(Command::Flush(tx))
            .map_err(|_| io::Error::other("json writer stopped"))?;
        rx.recv()
            .map_err(|_| io::Error::other("json writer stopped"))?
    }
}

impl Drop for JsonWriter {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

fn writer_loop(rx: Receiver<Command>, debounce: Duration) {
    let mut pending: HashMap<PathBuf, Encode> = HashMap::new();
    loop {
        let first = match rx.recv() {
            Ok(command) => command,
            Err(_) => {
                let _ = flush_pending(&mut pending);
                return;
            }
        };
        match first {
            Command::Save { path, encode } => {
                pending.insert(path, encode);
                let mut flush_now = false;
                if debounce.is_zero() {
                    flush_now = true;
                } else {
                    loop {
                        match rx.recv_timeout(debounce) {
                            Ok(Command::Save { path, encode }) => {
                                pending.insert(path, encode);
                            }
                            Ok(Command::Flush(reply)) => {
                                let result = flush_pending(&mut pending);
                                let _ = reply.send(result);
                                flush_now = true;
                                break;
                            }
                            Err(RecvTimeoutError::Timeout) => break,
                            Err(RecvTimeoutError::Disconnected) => {
                                let _ = flush_pending(&mut pending);
                                return;
                            }
                        }
                    }
                }
                if flush_now {
                    let _ = flush_pending(&mut pending);
                }
            }
            Command::Flush(reply) => {
                let result = flush_pending(&mut pending);
                let _ = reply.send(result);
            }
        }
    }
}

fn flush_pending(pending: &mut HashMap<PathBuf, Encode>) -> io::Result<()> {
    let mut first_error = None;
    for (path, encode) in pending.drain() {
        let result = std::panic::catch_unwind(AssertUnwindSafe(encode))
            .unwrap_or_else(|_| Err(io::Error::other("json serialization panicked")))
            .and_then(|bytes| chordal_core::fs::atomic_write(&path, &bytes));
        if let Err(error) = result {
            tracing::warn!(path = %path.display(), %error, "json persistence failed");
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
    }
    first_error.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "chordial-json-writer-{name}-{}-{}",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
    }

    use std::sync::atomic::{AtomicU64, Ordering};
    static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn coalesces_saves_and_flush_persists_latest_snapshot() {
        let writer = JsonWriter::with_debounce(Duration::from_secs(60));
        let path = temp_path("latest");
        writer.save_json(&path, json!({ "value": 1 }));
        writer.save_json(&path, json!({ "value": 2 }));
        writer.flush().unwrap();
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["value"], 2);
        writer.flush().unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn flush_is_ordered_after_prior_saves() {
        let writer = JsonWriter::with_debounce(Duration::ZERO);
        let path = temp_path("ordered");
        for value in 0..20 {
            writer.save_json(&path, json!({ "value": value }));
        }
        writer.flush().unwrap();
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["value"], 19);
        let _ = std::fs::remove_file(path);
    }
}
