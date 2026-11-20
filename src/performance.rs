//! Real, bounded on-demand process-tree telemetry for Settings -> Performance.
//!
//! Designed with zero idle overhead: sampling is ONLY invoked on request while the
//! Settings window is open on the Performance section, and uses direct Win32 ToolHelp +
//! PSAPI calls (< 0.5 ms per sample) instead of spawning PowerShell/WMI processes.

use std::time::Instant;

use serde::Serialize;

/// Live process-tree performance snapshot displayed in Settings -> Performance.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PerformanceMetrics {
    pub rust_private_bytes: u64,
    pub rust_working_set_bytes: u64,
    pub total_private_bytes: u64,
    pub total_working_set_bytes: u64,
    pub webview_process_count: usize,
    pub total_process_count: usize,
    pub cpu_percent: f64,
    pub webview_low_memory: bool,
    pub webview_suspended: bool,
    pub runtime_version: String,
}

/// Stateful sampler computing delta CPU percentage across bounded samples.
pub struct PerformanceSampler {
    last_sample_time: Option<Instant>,
    last_total_cpu_100ns: u64,
}

impl PerformanceSampler {
    pub fn new() -> Self {
        Self {
            last_sample_time: None,
            last_total_cpu_100ns: 0,
        }
    }

    pub fn reset(&mut self) {
        self.last_sample_time = None;
        self.last_total_cpu_100ns = 0;
    }

    pub fn sample(
        &mut self,
        webview_low_memory: bool,
        webview_suspended: bool,
        runtime_version: String,
    ) -> PerformanceMetrics {
        #[cfg(target_os = "windows")]
        {
            self.sample_windows(webview_low_memory, webview_suspended, runtime_version)
        }
        #[cfg(not(target_os = "windows"))]
        {
            PerformanceMetrics {
                webview_low_memory,
                webview_suspended,
                runtime_version,
                ..Default::default()
            }
        }
    }

    #[cfg(target_os = "windows")]
    fn sample_windows(
        &mut self,
        webview_low_memory: bool,
        webview_suspended: bool,
        runtime_version: String,
    ) -> PerformanceMetrics {
        use std::collections::{HashMap, HashSet};
        use std::mem::size_of;

        use windows::Win32::Foundation::{CloseHandle, FILETIME};
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        };
        use windows::Win32::System::ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX,
        };
        use windows::Win32::System::Threading::{
            GetCurrentProcessId, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_VM_READ,
        };

        let my_pid = unsafe { GetCurrentProcessId() };
        let mut children_map: HashMap<u32, Vec<u32>> = HashMap::new();

        unsafe {
            if let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
                let mut entry = PROCESSENTRY32W {
                    dwSize: size_of::<PROCESSENTRY32W>() as u32,
                    ..Default::default()
                };
                if Process32FirstW(snap, &mut entry).is_ok() {
                    loop {
                        children_map
                            .entry(entry.th32ParentProcessID)
                            .or_default()
                            .push(entry.th32ProcessID);
                        if Process32NextW(snap, &mut entry).is_err() {
                            break;
                        }
                    }
                }
                let _ = CloseHandle(snap);
            }
        }

        // BFS from our own PID to collect all descendant WebView2 processes.
        let mut tree_pids: HashSet<u32> = HashSet::new();
        let mut stack = vec![my_pid];
        while let Some(pid) = stack.pop() {
            if tree_pids.insert(pid)
                && let Some(kids) = children_map.get(&pid)
            {
                stack.extend(kids.iter().copied());
            }
        }

        let mut rust_private = 0u64;
        let mut rust_ws = 0u64;
        let mut total_private = 0u64;
        let mut total_ws = 0u64;
        let mut total_cpu_100ns = 0u64;
        let mut counted = 0usize;

        for &pid in &tree_pids {
            unsafe {
                let handle = OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
                    false,
                    pid,
                )
                .or_else(|_| OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid));
                let Ok(handle) = handle else {
                    continue;
                };

                let mut pmc = PROCESS_MEMORY_COUNTERS_EX {
                    cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
                    ..Default::default()
                };
                if GetProcessMemoryInfo(
                    handle,
                    &mut pmc as *mut PROCESS_MEMORY_COUNTERS_EX as *mut _,
                    size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
                )
                .is_ok()
                {
                    let priv_b = pmc.PrivateUsage as u64;
                    let ws_b = pmc.WorkingSetSize as u64;
                    total_private = total_private.saturating_add(priv_b);
                    total_ws = total_ws.saturating_add(ws_b);
                    if pid == my_pid {
                        rust_private = priv_b;
                        rust_ws = ws_b;
                    }
                    counted += 1;
                }

                let (mut c, mut e, mut k, mut u) = (
                    FILETIME::default(),
                    FILETIME::default(),
                    FILETIME::default(),
                    FILETIME::default(),
                );
                if GetProcessTimes(handle, &mut c, &mut e, &mut k, &mut u).is_ok() {
                    let kt = ((k.dwHighDateTime as u64) << 32) | (k.dwLowDateTime as u64);
                    let ut = ((u.dwHighDateTime as u64) << 32) | (u.dwLowDateTime as u64);
                    total_cpu_100ns = total_cpu_100ns.saturating_add(kt).saturating_add(ut);
                }

                let _ = CloseHandle(handle);
            }
        }

        let now = Instant::now();
        let cpu_percent = if let Some(prev_time) = self.last_sample_time {
            let elapsed_secs = now.duration_since(prev_time).as_secs_f64();
            if elapsed_secs > 0.05 && total_cpu_100ns >= self.last_total_cpu_100ns {
                let delta_secs =
                    (total_cpu_100ns - self.last_total_cpu_100ns) as f64 / 10_000_000.0;
                let logical_cpus = std::thread::available_parallelism()
                    .map(|n| n.get() as f64)
                    .unwrap_or(1.0);
                ((delta_secs / (elapsed_secs * logical_cpus)) * 100.0).clamp(0.0, 100.0)
            } else {
                0.0
            }
        } else {
            0.0
        };

        self.last_sample_time = Some(now);
        self.last_total_cpu_100ns = total_cpu_100ns;

        PerformanceMetrics {
            rust_private_bytes: rust_private,
            rust_working_set_bytes: rust_ws,
            total_private_bytes: total_private,
            total_working_set_bytes: total_ws,
            webview_process_count: counted.saturating_sub(1),
            total_process_count: counted,
            cpu_percent,
            webview_low_memory,
            webview_suspended,
            runtime_version,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampler_measures_current_process_without_error() {
        let mut sampler = PerformanceSampler::new();
        let m = sampler.sample(false, false, "test".into());
        #[cfg(target_os = "windows")]
        {
            assert!(m.total_process_count >= 1);
            assert!(m.rust_private_bytes > 0);
            assert!(m.total_private_bytes >= m.rust_private_bytes);
        }
    }
}
