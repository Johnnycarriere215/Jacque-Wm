//! CPU utilisation probe.
//!
//! Uses `GetSystemTimes`. We sample twice and report the ratio
//! (kernel + user − idle) / (kernel + user) as the percent.
//! A 5-sample rolling mean smooths the result.

use std::time::Instant;

use windows::Win32::System::SystemInformation::{
    GetSystemTimes, FILETIME,
};

use crate::core::metrics::{CpuSample, RollingMean};

pub struct CpuProbe {
    last_idle: u64,
    last_kernel: u64,
    last_user: u64,
    last_read: Option<Instant>,
    rolling: RollingMean,
    cores: u32,
}

impl CpuProbe {
    pub fn new() -> Self {
        Self {
            last_idle: 0,
            last_kernel: 0,
            last_user: 0,
            last_read: None,
            rolling: RollingMean::new(5),
            cores: num_cpus(),
        }
    }

    pub fn sample(&mut self) -> CpuSample {
        unsafe {
            let mut idle = FILETIME::default();
            let mut kernel = FILETIME::default();
            let mut user = FILETIME::default();
            let ok = GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user));
            if !ok.as_bool() {
                return CpuSample {
                    percent: 0.0,
                    cores: self.cores,
                    sampled_at: Instant::now(),
                };
            }

            let idle_v = filetime_to_u64(&idle);
            let kern_v = filetime_to_u64(&kernel);
            let user_v = filetime_to_u64(&user);

            let now = Instant::now();
            let percent = if let Some(last) = self.last_read {
                let dt = now.duration_since(last).as_secs_f32().max(0.001);
                let d_idle = idle_v.saturating_sub(self.last_idle) as f32 / dt;
                let d_kernel = kern_v.saturating_sub(self.last_kernel) as f32 / dt;
                let d_user = user_v.saturating_sub(self.last_user) as f32 / dt;
                let total = d_kernel + d_user;
                if total <= 0.0001 {
                    self.last_read = Some(now);
                    self.last_idle = idle_v;
                    self.last_kernel = kern_v;
                    self.last_user = user_v;
                    return CpuSample {
                        percent: 0.0,
                        cores: self.cores,
                        sampled_at: now,
                    };
                }
                let busy = (d_kernel + d_user) - d_idle;
                let pct = (busy / total) * 100.0;
                self.last_read = Some(now);
                self.last_idle = idle_v;
                self.last_kernel = kern_v;
                self.last_user = user_v;
                self.rolling.push_and_average(pct.clamp(0.0, 100.0))
            } else {
                self.last_read = Some(now);
                self.last_idle = idle_v;
                self.last_kernel = kern_v;
                self.last_user = user_v;
                0.0
            };
            CpuSample {
                percent,
                cores: self.cores,
                sampled_at: Instant::now(),
            }
        }
    }
}

fn filetime_to_u64(ft: &FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | (ft.dwLowDateTime as u64)
}

fn num_cpus() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(1)
}

impl Default for CpuProbe {
    fn default() -> Self {
        Self::new()
    }
}
