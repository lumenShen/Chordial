use async_task::Runnable;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

use crate::{PlatformDispatcher, TaskLabel};

use super::bridge::AndroidEvent;

pub(crate) enum PlatformMessage {
    Runnable(Runnable),
    Android(AndroidEvent),
}

struct TimerEntry {
    deadline: Instant,
    sequence: u64,
    runnable: Runnable,
}

impl PartialEq for TimerEntry {
    fn eq(&self, other: &Self) -> bool {
        self.deadline == other.deadline && self.sequence == other.sequence
    }
}

impl Eq for TimerEntry {}

impl PartialOrd for TimerEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TimerEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .deadline
            .cmp(&self.deadline)
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

pub(crate) struct AndroidDispatcher {
    main_sender: Sender<PlatformMessage>,
    background_sender: flume::Sender<Runnable>,
    timer_sender: Sender<TimerEntry>,
    main_thread_id: ThreadId,
    _background_threads: Vec<thread::JoinHandle<()>>,
}

impl AndroidDispatcher {
    pub(crate) fn new() -> (Self, Receiver<PlatformMessage>) {
        let (main_sender, main_receiver) = mpsc::channel();
        let (background_sender, background_receiver) = flume::unbounded();
        let worker_count = std::thread::available_parallelism()
            .map(|value| value.get())
            .unwrap_or(1)
            .min(4);
        let mut background_threads = Vec::with_capacity(worker_count);
        for index in 0..worker_count {
            let receiver: flume::Receiver<Runnable> = background_receiver.clone();
            background_threads.push(
                thread::Builder::new()
                    .name(format!("chordial-worker-{index}"))
                    .spawn(move || {
                        for runnable in receiver {
                            runnable.run();
                        }
                    })
                    .expect("failed to spawn Android worker"),
            );
        }

        let (timer_sender, timer_receiver) = mpsc::channel();
        let timer_main_sender = main_sender.clone();
        let timer_thread = thread::Builder::new()
            .name("chordial-timer".to_owned())
            .spawn(move || {
                let mut queue = BinaryHeap::new();
                let mut sequence = 0;
                loop {
                    let next_deadline = queue.peek().map(|entry: &TimerEntry| entry.deadline);
                    let message = match next_deadline {
                        Some(deadline) => timer_receiver
                            .recv_timeout(deadline.saturating_duration_since(Instant::now())),
                        None => match timer_receiver.recv() {
                            Ok(entry) => Ok(entry),
                            Err(_) => return,
                        },
                    };
                    match message {
                        Ok(entry) => {
                            sequence += 1;
                            queue.push(TimerEntry { sequence, ..entry });
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            let now = Instant::now();
                            while queue.peek().is_some_and(|entry| entry.deadline <= now) {
                                let entry = queue.pop().expect("timer queue");
                                if timer_main_sender
                                    .send(PlatformMessage::Runnable(entry.runnable))
                                    .is_err()
                                {
                                    return;
                                }
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }
            })
            .expect("failed to spawn Android timer thread");
        background_threads.push(timer_thread);

        (
            Self {
                main_sender,
                background_sender,
                timer_sender,
                main_thread_id: thread::current().id(),
                _background_threads: background_threads,
            },
            main_receiver,
        )
    }

    pub(crate) fn sender(&self) -> Sender<PlatformMessage> {
        self.main_sender.clone()
    }
}

impl PlatformDispatcher for AndroidDispatcher {
    fn is_main_thread(&self) -> bool {
        thread::current().id() == self.main_thread_id
    }

    fn dispatch(&self, runnable: Runnable, _label: Option<TaskLabel>) {
        let _ = self.background_sender.send(runnable);
    }

    fn dispatch_on_main_thread(&self, runnable: Runnable) {
        if let Err(error) = self.main_sender.send(PlatformMessage::Runnable(runnable))
            && let PlatformMessage::Runnable(runnable) = error.0
        {
            // A !Send foreground future must be forgotten rather than dropped on a worker.
            std::mem::forget(runnable);
        }
    }

    fn dispatch_after(&self, duration: Duration, runnable: Runnable) {
        let _ = self.timer_sender.send(TimerEntry {
            deadline: Instant::now() + duration,
            sequence: 0,
            runnable,
        });
    }
}
