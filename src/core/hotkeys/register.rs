//! Hotkey registration trait.
//!
//! Defines an OS-agnostic interface that the platform layer fulfils.
//! The platform produces [`HotkeyPress`] events and pushes them into
//! a channel; the dispatcher consumes the channel.

use std::sync::mpsc;

use crate::core::hotkeys::keys::HotkeyPress;
use crate::error::Result;

/// Source of keyboard events. The platform layer implements this trait.
///
/// Implementations are expected to be cheap — the dispatcher is the one
/// that decides what to do with the events.
pub trait HotkeySource: Send + Sync {
    /// Start pushing events into the channel. Returns `Ok(())` once
    /// the hook is installed.
    fn install(&self) -> Result<()>;
    /// Stop pushing events. Safe to call multiple times.
    fn uninstall(&self) -> Result<()>;
}

/// Channel-like push interface used by the platform source.
pub trait HotkeySink: Send + Sync {
    /// Push a press event into the sink. Implementations must be cheap
    /// and non-blocking — pushing is called from the keyboard hook
    /// callback, which has a 300 ms Windows-imposed timeout.
    fn push(&self, press: HotkeyPress);
}

/// Bounded sink backed by `std::sync::mpsc::SyncSender`. The bound is
/// small (256) so the keyboard hook never blocks the OS deadline.
pub struct ChannelSink {
    inner: mpsc::SyncSender<HotkeyPress>,
}

impl ChannelSink {
    /// Create a new sink from an `mpsc::SyncSender`.
    pub fn new(sender: mpsc::SyncSender<HotkeyPress>) -> Self {
        Self { inner: sender }
    }
}

impl HotkeySink for ChannelSink {
    fn push(&self, press: HotkeyPress) {
        // `try_send` is intentional: if the consumer is slow we'd
        // rather drop events than block the hook.
        let _ = self.inner.try_send(press);
    }
}

/// Convenience: build a (sync sender, receiver) pair with the
/// canonical capacity. Mirrors what the platform layer expects.
pub fn channel_pair(capacity: usize) -> (mpsc::SyncSender<HotkeyPress>, mpsc::Receiver<HotkeyPress>) {
    mpsc::sync_channel(capacity)
}
