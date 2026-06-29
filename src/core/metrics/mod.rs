//! Shared metric data types used by the top panel and the metrics
//! collector.
//!
//! All OS-specific counters live under `platform::windows::metrics::*`.
//! The types declared here are platform-agnostic so tests can construct
//! them without touching Win32.

use std::time::Instant;

/// Smoothed CPU utilization (0..=100).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CpuSample {
    /// Smoothed utilisation in percent (0..=100).
    pub percent: f32,
    /// Number of cores the figure was averaged over.
    pub cores: u32,
    /// Time the sample was taken.
    pub sampled_at: Instant,
}

impl Default for CpuSample {
    fn default() -> Self {
        Self {
            percent: 0.0,
            cores: 1,
            sampled_at: Instant::now(),
        }
    }
}

/// GPU utilisation. `presentable` is `true` when a real number could
/// be read; otherwise the panel should show `GPU --`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GpuSample {
    /// `true` if `percent` is meaningful.
    pub presentable: bool,
    /// 0..=100, valid only if `presentable`.
    pub percent: f32,
    /// Friendly name of the primary rendering adapter.
    pub adapter_name: Option<String>,
    /// Time the sample was taken.
    pub sampled_at: Instant,
}

impl Default for GpuSample {
    fn default() -> Self {
        Self {
            presentable: false,
            percent: 0.0,
            adapter_name: None,
            sampled_at: Instant::now(),
        }
    }
}

/// RAM usage in bytes plus derived percentage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RamSample {
    /// Total physical memory in bytes.
    pub total_bytes: u64,
    /// Used physical memory in bytes (`total_bytes - avail_bytes`).
    pub used_bytes: u64,
    /// 0..=100.
    pub percent: f32,
    pub sampled_at: Instant,
}

impl Default for RamSample {
    fn default() -> Self {
        Self {
            total_bytes: 0,
            used_bytes: 0,
            percent: 0.0,
            sampled_at: Instant::now(),
        }
    }
}

/// Network throughput sample (bytes/sec). `link_down == true` => no
/// connection was detected on any active interface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NetSample {
    /// `true` if no active interface is producing meaningful
    /// throughput.
    pub link_down: bool,
    /// Download bytes per second (rolling-averaged).
    pub rx_bytes_per_sec: u64,
    /// Upload bytes per second (rolling-averaged).
    pub tx_bytes_per_sec: u64,
    pub sampled_at: Instant,
}

impl Default for NetSample {
    fn default() -> Self {
        Self {
            link_down: true,
            rx_bytes_per_sec: 0,
            tx_bytes_per_sec: 0,
            sampled_at: Instant::now(),
        }
    }
}

/// Trivial rolling-average helper.
///
/// Holds the last `N` samples and yields the arithmetic mean when
/// `push_and_average` is called. Used to smooth CPU / GPU / network
/// counters that otherwise jitter.
#[derive(Debug, Clone)]
pub struct RollingMean {
    capacity: usize,
    buf: Vec<f32>,
}

impl RollingMean {
    /// Build a new rolling mean with the given capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            buf: Vec::with_capacity(capacity.max(1)),
        }
    }

    /// Add a sample and return the new mean of all stored samples.
    pub fn push_and_average(&mut self, sample: f32) -> f32 {
        if self.buf.len() == self.capacity {
            self.buf.remove(0);
        }
        self.buf.push(sample);
        let sum: f32 = self.buf.iter().copied().sum();
        sum / self.buf.len() as f32
    }

    /// Reset the buffer.
    pub fn clear(&mut self) {
        self.buf.clear();
    }

    /// Number of samples currently held.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// `true` if no samples are held.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolling_mean_smooths_input() {
        let mut r = RollingMean::new(3);
        assert_eq!(r.push_and_average(100.0), 100.0);
        assert_eq!(r.push_and_average(50.0), 75.0);
        assert_eq!(r.push_and_average(0.0), 50.0);
        // Fourth sample: oldest (100) is dropped -> [50, 0, 25] mean 25
        assert_eq!(r.push_and_average(25.0), 25.0);
    }

    #[test]
    fn defaults_are_safe_to_display() {
        let cpu = CpuSample::default();
        assert_eq!(cpu.cores, 1);
        assert_eq!(cpu.percent, 0.0);

        let gpu = GpuSample::default();
        assert!(!gpu.presentable);

        let ram = RamSample::default();
        assert_eq!(ram.total_bytes, 0);

        let net = NetSample::default();
        assert!(net.link_down);
    }
}
