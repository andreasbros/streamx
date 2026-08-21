//! Bridge between tokio (required by reqwest) and the GPUI async executor.
//!
//! GPUI has its own executor (`cx.background_executor()`) which is NOT tokio.
//! reqwest requires a tokio runtime. We launch a dedicated tokio Runtime on a
//! worker thread at startup, then provide `spawn()` which accepts any
//! `Future<Output = T> + Send + 'static` and returns a future resolvable
//! inside `cx.spawn`.

use once_cell::sync::OnceCell;
use std::future::Future;
use tokio::runtime::{Builder, Handle};
use tokio::sync::oneshot;

static TOKIO_HANDLE: OnceCell<Handle> = OnceCell::new();

/// Must be called once at program start (main.rs, before `Application::run`).
pub fn init() {
    TOKIO_HANDLE.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("streamx-tokio".into())
            .spawn(move || {
                let rt = Builder::new_multi_thread()
                    .enable_all()
                    .thread_name("streamx-tokio-worker")
                    .build()
                    .expect("failed to build tokio runtime");
                tx.send(rt.handle().clone()).expect("send handle");
                // Block forever; the Runtime lives as long as the thread.
                rt.block_on(std::future::pending::<()>());
            })
            .expect("spawn tokio thread");
        rx.recv().expect("tokio handle")
    });
}

/// Submit a future to the tokio runtime. Returns a future that resolves with
/// the result on the calling executor (typically a `cx.spawn` task on GPUI's
/// own executor).
pub fn spawn<F, T>(fut: F) -> impl Future<Output = T>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let handle = TOKIO_HANDLE
        .get()
        .expect("runtime::init() not called")
        .clone();
    let (tx, rx) = oneshot::channel::<T>();
    handle.spawn(async move {
        let v = fut.await;
        let _ = tx.send(v);
    });
    async move { rx.await.expect("tokio task was cancelled before sending") }
}

/// Fire-and-forget variant of [`spawn`]: the result is discarded and
/// nothing is left for the caller to await.
pub fn spawn_detached<F, T>(fut: F)
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let handle = TOKIO_HANDLE
        .get()
        .expect("runtime::init() not called")
        .clone();
    handle.spawn(async move {
        let _ = fut.await;
    });
}

/// Block the current thread on a future running on the tokio runtime.
/// Safe to call from GPUI's background threads (they are not tokio
/// worker threads). Used by the AssetSource, whose `load` method is
/// synchronous but needs to await an async fetch.
pub fn block_on<F, T>(fut: F) -> T
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let handle = TOKIO_HANDLE
        .get()
        .expect("runtime::init() not called")
        .clone();
    let (tx, rx) = std::sync::mpsc::channel::<T>();
    handle.spawn(async move {
        let v = fut.await;
        let _ = tx.send(v);
    });
    rx.recv().expect("tokio task was cancelled before sending")
}
