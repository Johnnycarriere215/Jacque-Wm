//! Metrics collection orchestrator.
//!
//! Public re-exports of the four metric modules. The orchestrator
//! pulls a snapshot from each subsystem on demand — callers (the
//! panel render thread) call [`MetricsCollector::snapshot`] once a
//! second.

pub mod cpu;
pub mod gpu;
pub mod network;
pub mod ram;

use std::sync::Arc;

use parking_lot::Mutex;

use crate::core::metrics::{CpuSample, GpuSample, NetSample, RamSample};

/// Single entry point for collecting every metric.
pub struct MetricsCollector {
    cpu: cpu::CpuProbe,
    ram: ram::RamProbe,
    net: network::NetProbe,
    gpu: gpu::GpuProbe,
}

impl MetricsCollector {
    /// Build a fresh collector. Allocates a PDH query for GPU if
    /// available — fails back to "GPU --" otherwise.
    pub fn new() -> Self {
        Self {
            cpu: cpu::CpuProbe::new(),
            ram: ram::RamProbe::new(),
            net: network::NetProbe::new(),
            gpu: gpu::GpuProbe::new(),
        }
    }

    /// One-shot sample.
    pub fn snapshot(&mut self) -> (CpuSample, GpuSample, RamSample, NetSample) {
        let cpu = self.cpu.sample();
        let gpu = self.gpu.sample();
        let ram = self.ram.sample();
        let net = self.net.sample();
        (cpu, gpu, ram, net)
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

// A thread-safe shared handle so the panel thread can read.
pub type SharedMetrics = Arc<Mutex<MetricsCollector>>;

/// Build a thread-safe collector wrapped in Arc<Mutex<…>>.
pub fn shared() -> SharedMetrics {
    Arc::new(Mutex::new(MetricsCollector::new()))
}
