//! RAM probe.
//!
//! Reads `GlobalMemoryStatusEx` on each call. Cheap; no caching.

use std::time::Instant;

use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

use crate::core::metrics::RamSample;

pub struct RamProbe;

impl RamProbe {
    pub fn new() -> Self {
        Self
    }

    pub fn sample(&self) -> RamSample {
        unsafe {
            let mut info = MEMORYSTATUSEX {
                dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
                ..Default::default()
            };
            let ok = GlobalMemoryStatusEx(&mut info);
            if !ok.as_bool() {
                return RamSample::default();
            }
            let total = info.ullTotalPhys;
            let avail = info.ullAvailPhys;
            let used = total.saturating_sub(avail);
            let pct = if total == 0 {
                0.0
            } else {
                (used as f32 / total as f32) * 100.0
            };
            RamSample {
                total_bytes: total,
                used_bytes: used,
                percent: pct,
                sampled_at: Instant::now(),
            }
        }
    }
}

impl Default for RamProbe {
    fn default() -> Self {
        Self
    }
}
