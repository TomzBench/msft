//! common

pub mod cleanup;
pub mod environment;
pub mod instance;

pub use cleanup::ThreadpoolCleanupGroup;
pub use environment::{ThreadpoolHandle, ThreadpoolCallbackEnvironment, ThreadpoolPriority};
pub use instance::ThreadpoolCallbackInstance;

/// Wait for pending threadpool callbacks, or cancel pending threadpool callbacks
#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WaitPending {
    /// Wait for pending threadpool callbacks
    Wait = 0,
    /// Cancel pending threadpool callbacks
    Cancel = 1,
}
