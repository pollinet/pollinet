//! Async runtime management for FFI
//! 
//! Maintains a single-threaded Tokio runtime that is initialized once and
//! used for all async operations from the FFI boundary.

use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use std::sync::Arc;
use tokio::runtime::Runtime;

static RUNTIME: OnceCell<Arc<Mutex<Runtime>>> = OnceCell::new();

/// Initialize the global async runtime
pub fn init_runtime() -> Result<(), String> {
    // Use a multi-threaded runtime with a small worker pool
    // This is needed for spawn_blocking support (e.g., RPC calls)
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2) // Keep it lightweight for Android
        .enable_all()
        .build()
        .map_err(|e| format!("Failed to create runtime: {}", e))?;
    
    RUNTIME
        .set(Arc::new(Mutex::new(runtime)))
        .map_err(|_| "Runtime already initialized".to_string())
}

/// Get a reference to the global runtime
pub fn get_runtime() -> Result<Arc<Mutex<Runtime>>, String> {
    RUNTIME
        .get()
        .cloned()
        .ok_or_else(|| "Runtime not initialized".to_string())
}

/// Execute an async task on the global runtime
pub fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let runtime = get_runtime().expect("Runtime not initialized");
    let rt = runtime.lock();
    rt.block_on(future)
}

/// Spawn a task on the global runtime
pub fn spawn<F>(future: F) -> tokio::task::JoinHandle<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    let runtime = get_runtime().expect("Runtime not initialized");
    let rt = runtime.lock();
    rt.spawn(future)
}

