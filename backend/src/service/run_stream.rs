//! Replayable chunk stream for one chat run.
//!
//! The SSE response of the original request consumes an `mpsc` channel, which
//! dies with the client connection. A `RunStream` keeps a copy of every chunk
//! and fans them out to live subscribers, so a reloaded tab can reconnect to
//! `GET /api/chat/resume` and replay the stream from the beginning.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

use crate::aistream::Chunk;

#[derive(Debug, Clone)]
pub struct SeqChunk {
    pub seq: u64,
    pub chunk: Chunk,
}

pub struct RunStream {
    buffer: Mutex<Vec<SeqChunk>>,
    tx: Mutex<Option<broadcast::Sender<SeqChunk>>>,
    next_seq: AtomicU64,
}

impl Default for RunStream {
    fn default() -> Self {
        Self::new()
    }
}

impl RunStream {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self {
            buffer: Mutex::new(Vec::new()),
            tx: Mutex::new(Some(tx)),
            // Sequence starts at 1 so "no buffered chunks yet" (0) is distinct.
            next_seq: AtomicU64::new(1),
        }
    }

    pub fn publish(&self, chunk: Chunk) {
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        let item = SeqChunk { seq, chunk };
        {
            let mut buffer = self.buffer.lock().expect("run stream buffer poisoned");
            buffer.push(item.clone());
        }
        if let Some(tx) = self.tx.lock().expect("run stream sender poisoned").as_ref() {
            let _ = tx.send(item);
        }
    }

    /// End live delivery. The buffered chunks stay replayable for late
    /// subscribers (e.g. a tab that reconnects just after the run finished).
    pub fn finish(&self) {
        self.tx.lock().expect("run stream sender poisoned").take();
    }

    /// Subscribe and replay everything produced so far, then follow live.
    pub fn subscribe(self: &Arc<Self>) -> RunSubscription {
        let rx = self
            .tx
            .lock()
            .expect("run stream sender poisoned")
            .as_ref()
            .map(|tx| tx.subscribe());
        let buffered = self
            .buffer
            .lock()
            .expect("run stream buffer poisoned")
            .clone();
        let last_seq = buffered.last().map(|item| item.seq).unwrap_or(0);
        RunSubscription {
            stream: Arc::clone(self),
            buffered: buffered.into_iter(),
            rx,
            last_seq,
        }
    }
}

pub struct RunSubscription {
    stream: Arc<RunStream>,
    buffered: std::vec::IntoIter<SeqChunk>,
    rx: Option<broadcast::Receiver<SeqChunk>>,
    last_seq: u64,
}

impl RunSubscription {
    /// Next chunk, or `None` once the run finished and the buffer is drained.
    pub async fn next(&mut self) -> Option<SeqChunk> {
        loop {
            if let Some(item) = self.buffered.next() {
                self.last_seq = item.seq;
                return Some(item);
            }
            let rx = self.rx.as_mut()?;
            match rx.recv().await {
                // A chunk may appear both in the replay buffer and the live
                // channel when it raced the subscription — skip duplicates.
                Ok(item) if item.seq > self.last_seq => {
                    self.last_seq = item.seq;
                    return Some(item);
                }
                Ok(_) => continue,
                // Slow subscriber: catch up from the replay buffer instead of
                // dropping deltas (that would corrupt the replayed text).
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    let pending: Vec<SeqChunk> = {
                        let buffer = self
                            .stream
                            .buffer
                            .lock()
                            .expect("run stream buffer poisoned");
                        buffer
                            .iter()
                            .filter(|item| item.seq > self.last_seq)
                            .cloned()
                            .collect()
                    };
                    self.buffered = pending.into_iter();
                }
                Err(broadcast::error::RecvError::Closed) => {
                    self.rx = None;
                }
            }
        }
    }
}
