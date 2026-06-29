//! Network throughput probe.
//!
//! Reads `GetIfTable` from IPHLPAPI. Computes per-second delta for
//! `dwInOctets` / `dwOutOctets`. Filters out loopback and disabled
//! interfaces. Smooths with a 3-sample rolling mean.

use std::time::Instant;

use windows::Win32::NetworkManagement::IpHelper::{GetIfTable, MIB_IFROW};
use crate::core::metrics::{NetSample, RollingMean};

pub struct NetProbe {
    last_rx: u64,
    last_tx: u64,
    last_read: Option<Instant>,
    rx_smooth: RollingMean,
    tx_smooth: RollingMean,
}

impl NetProbe {
    pub fn new() -> Self {
        Self {
            last_rx: 0,
            last_tx: 0,
            last_read: None,
            rx_smooth: RollingMean::new(3),
            tx_smooth: RollingMean::new(3),
        }
    }

    pub fn sample(&mut self) -> NetSample {
        unsafe {
            let mut size: u32 = 0;
            // First call to discover required size.
            let _ = GetIfTable(None, &mut size, false);
            let mut buf = vec![0u8; size as usize];
            let ptr = buf.as_mut_ptr() as *mut MIB_IFROW;
            // SAFETY: MIB_IFROW has the same prefix as MIB_IFTABLE here, but we treat
            // the buffer as a MIB_IFTABLE-of-rows.
            let result = GetIfTable(Some(buf.as_mut_ptr().cast()), &mut size, false);
            if result != 0 {
                return NetSample {
                    link_down: true,
                    sampled_at: Instant::now(),
                    ..Default::default()
                };
            }
            let table = &*(buf.as_ptr() as *const MibIfTable);
            let now = Instant::now();
            let mut total_rx: u64 = 0;
            let mut total_tx: u64 = 0;
            let mut any_active = false;
            for i in 0..table.dwNumEntries as usize {
                let row_ptr = (table.table.as_ptr() as *const u8)
                    .add(i * std::mem::size_of::<MIB_IFROW>())
                    as *const MIB_IFROW;
                let row = &*row_ptr;
                if (row.dwAdminStatus != 1) || (row.dwOperStatus != 1) {
                    // IF_ADMIN_STATUS_UP and IF_OPER_STATUS_UP = 1.
                    continue;
                }
                if row.dwType == 24 {
                    // IF_TYPE_SOFTWARE_LOOPBACK — skip.
                    continue;
                }
                if row.dwInOctets == 0 && row.dwOutOctets == 0 {
                    continue;
                }
                total_rx = total_rx.saturating_add(row.dwInOctets as u64);
                total_tx = total_tx.saturating_add(row.dwOutOctets as u64);
                any_active = true;
            }
            if !any_active {
                // Reset baseline so we don't claim absurd speeds.
                self.last_read = Some(now);
                self.last_rx = total_rx;
                self.last_tx = total_tx;
                return NetSample {
                    link_down: true,
                    sampled_at: now,
                    ..Default::default()
                };
            }
            let (rx_psec, tx_psec) = if let Some(last) = self.last_read {
                let dt = now.duration_since(last).as_secs_f32().max(0.001);
                let rx = (total_rx.saturating_sub(self.last_rx) as f32 / dt) as u64;
                let tx = (total_tx.saturating_sub(self.last_tx) as f32 / dt) as u64;
                self.last_read = Some(now);
                self.last_rx = total_rx;
                self.last_tx = total_tx;
                (rx, tx)
            } else {
                self.last_read = Some(now);
                self.last_rx = total_rx;
                self.last_tx = total_tx;
                (0, 0)
            };
            let rx_smoothed = self.rx_smooth.push_and_average(rx_psec as f32) as u64;
            let tx_smoothed = self.tx_smooth.push_and_average(tx_psec as f32) as u64;
            NetSample {
                link_down: false,
                rx_bytes_per_sec: rx_smoothed,
                tx_bytes_per_sec: tx_smoothed,
                sampled_at: now,
            }
        }
    }
}

#[repr(C)]
struct MibIfTable {
    dwNumEntries: u32,
    table: [MIB_IFROW; 1],
}

impl Default for NetProbe {
    fn default() -> Self {
        Self::new()
    }
}
