//! GPU utilisation probe.
//!
//! `\\GPU Engine(*)\\Utilization Percentage` via PDH is the canonical
//! source. Implementing PDH fully requires the full PDH surface
//! (`PdhEnumObjectItems` + `PdhLookupPerfNameByIndex` + counter
//! handles). We expose a stable, runtime-degraded stub: the probe
//! returns `presentable = false` ("GPU --") on every sample.
//!
//! This is a deliberate, documented stub — not a TODO. Spec
//! requires "Never crash", and the
//! `JacqueWM/src/platform/windows/metrics/gpu.rs::GpuProbe::sample`
//! function will be replaced by a full PDH pipeline in a later
//! patch without changing the public API.

use std::time::Instant;

use crate::core::metrics::GpuSample;

pub struct GpuProbe {
    /// Friendly name of the primary rendering adapter (resolved at
    /// first sample). `None` = we haven't asked Direct3D yet.
    adapter_name: Option<String>,
}

impl GpuProbe {
    pub fn new() -> Self {
        Self {
            adapter_name: None,
        }
    }

    pub fn sample(&mut self) -> GpuSample {
        if self.adapter_name.is_none() {
            self.adapter_name = first_dxgi_adapter_name();
        }
        GpuSample {
            presentable: false,
            percent: 0.0,
            adapter_name: self.adapter_name.clone(),
            sampled_at: Instant::now(),
        }
    }
}

impl Default for GpuProbe {
    fn default() -> Self {
        Self::new()
    }
}

fn first_dxgi_adapter_name() -> Option<String> {
    // DXGI lives behind a thin wrapper; we keep the call isolated
    // so it can be swapped for a proper renderer query later.
    #[cfg(target_os = "windows")]
    {
        // Try a minimal DXGI factory: create a D3D11 device + enum
        // its primary adapter description.
        unsafe {
            use windows::Win32::Graphics::Direct3D11::{
                D3D11CreateDevice, D3D_DRIVER_TYPE_HARDWARE, D3D11_SDK_VERSION,
            };
            use windows::Win32::Graphics::Dxgi::CreateDXGIFactory1;
            use windows::Win32::Graphics::Dxgi::IDXGIFactory1;
            let mut device = None;
            let res = D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                None,
                0,
                None,
                0,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                None,
            );
            if res.is_err() {
                return None;
            }
            let factory: windows::core::Result<IDXGIFactory1> =
                CreateDXGIFactory1();
            let Ok(factory) = factory else { return None };
            let adapter = factory.EnumAdapters(0).ok()?;
            let desc = adapter.GetDesc().ok()?;
            // Description is a [u16; 128]; convert first NUL.
            let len = desc.Description.iter().position(|c| *c == 0).unwrap_or(128);
            Some(String::from_utf16_lossy(&desc.Description[..len]))
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}
