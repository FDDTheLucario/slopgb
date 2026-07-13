//! The stop/finished shutdown machinery shared by every background socket
//! worker ([`crate::link::LinkSocket`], [`crate::mcp::server::Server`]): a
//! background thread that polls a `stop` flag to exit promptly, a `finished`
//! flag it always leaves set on exit (success *or* panic unwind, via a
//! drop-guard) so the owner can reap a worker that died on its own, and a
//! `Drop` that requests the stop and joins the thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

/// A background worker thread plus its stop/finished flag pair. Construct with
/// [`Self::spawn`]; `Drop` signals `stop` and joins.
pub(crate) struct ReapedWorker {
    stop: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl ReapedWorker {
    /// Spawn `body` on a background thread, handing it the `stop` flag it must
    /// poll to exit. Marks `finished` when `body` returns *or* panics, so a
    /// panicked worker is still reapable via [`Self::is_finished`].
    pub(crate) fn spawn<F>(body: F) -> Self
    where
        F: FnOnce(Arc<AtomicBool>) + Send + 'static,
    {
        let stop = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let (body_stop, guard_finished) = (Arc::clone(&stop), Arc::clone(&finished));
        let handle = thread::spawn(move || {
            struct FinishGuard(Arc<AtomicBool>);
            impl Drop for FinishGuard {
                fn drop(&mut self) {
                    self.0.store(true, Ordering::Relaxed);
                }
            }
            let _guard = FinishGuard(guard_finished);
            body(body_stop);
        });
        Self {
            stop,
            finished,
            handle: Some(handle),
        }
    }

    /// Whether the worker thread has exited (success, panic, or already
    /// stopped-and-joined).
    #[must_use]
    pub(crate) fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Relaxed)
    }
}

impl Drop for ReapedWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}
