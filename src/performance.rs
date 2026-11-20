//! Real, bounded on-demand process-tree telemetry for Settings -> Performance.
//!
//! Design principles:
//! - Zero idle overhead: sampling runs ONLY on request while Settings -> Performance is open.
//! - Uses official Win32 ToolHelp + PSAPI (`GetProcessMemoryInfo` / `GetProcessTimes`) combined
//!   with Microsoft WebView2's documented `ICoreWebView2Environment8::GetProcessInfos()` role map.
//! - Reports Private Commit (`PrivateUsage`) per role to avoid double-counting shared DLLs.
//! - Computes both instantaneous (2s) and rolling 10-second averaged CPU % transparently.

use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use serde::Serialize;

const ROLLING_WINDOW_SAMPLES: usize = 5; // 5 × 2s = 10s rolling window

/// Aggregated memory usage for a single subprocess role (e.g., Renderer, GPU Process).
#[derive(Debug, Clone, Serialize)]
pub struct RoleMemory {
    pub role: String,
    pub count: usize,
    pub private_bytes: u64,
    pub working_set_bytes: u64,
}

/// Live process-tree performance snapshot displayed in Settings -> Performance.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PerformanceMetrics {
    pub rust_private_bytes: u64,
    pub rust_working_set_bytes: u64,
    pub total_private_bytes: u64,
    pub total_working_set_bytes: u64,
    pub webview_process_count: usize,
    pub renderer_count: usize,
    pub total_process_count: usize,
    /// Instantaneous CPU % over the latest sampling interval (~2s).
    pub cpu_percent: f64,
    /// Rolling average CPU % over up to the last 10 seconds.
    pub cpu_rolling_avg_percent: f64,
    pub sample_count: usize,
    pub window_secs: u64,
    pub webview_low_memory: bool,
    pub webview_suspended: bool,
    pub runtime_version: String,
    pub roles: Vec<RoleMemory>,
}

/// Stateful sampler computing instantaneous and rolling-average CPU % across bounded samples.
pub struct PerformanceSampler {
    last_sample_time: Option<Instant>,
    last_pid_cpu_100ns: HashMap<u32, u64>,
    cpu_history: VecDeque<f64>,
}

impl PerformanceSampler {
    pub fn new() -> Self {
        Self {
            last_sample_time: None,
            last_pid_cpu_100ns: HashMap::new(),
            cpu_history: VecDeque::with_capacity(ROLLING_WINDOW_SAMPLES),
        }
    }

    pub fn reset(&mut self) {
        self.last_sample_time = None;
        self.last_pid_cpu_100ns.clear();
        self.cpu_history.clear();
    }

    pub fn sample(
        &mut self,
        webview_low_memory: bool,
        webview_suspended: bool,
        runtime_version: String,
        role_map: &HashMap<u32, &'static str>,
    ) -> PerformanceMetrics {
        #[cfg(target_os = "windows")]
        {
            self.sample_windows(
                webview_low_memory,
                webview_suspended,
                runtime_version,
                role_map,
            )
        }
        #[cfg(target_os = "linux")]
        {
            let _ = role_map;
            self.sample_linux(webview_low_memory, webview_suspended, runtime_version)
        }
        #[cfg(target_os = "macos")]
        {
            let _ = role_map;
            self.sample_macos(webview_low_memory, webview_suspended, runtime_version)
        }
        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        {
            let _ = role_map;
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
        role_map: &HashMap<u32, &'static str>,
    ) -> PerformanceMetrics {
        use std::collections::HashSet;
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
        let mut direct_children: HashSet<u32> = HashSet::new();

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
                        if entry.th32ParentProcessID == my_pid {
                            direct_children.insert(entry.th32ProcessID);
                        }
                        if Process32NextW(snap, &mut entry).is_err() {
                            break;
                        }
                    }
                }
                let _ = CloseHandle(snap);
            }
        }

        // BFS from our PID + union with official WebView2 GetProcessInfos PIDs.
        let mut tree_pids: HashSet<u32> = HashSet::new();
        let mut stack = vec![my_pid];
        while let Some(pid) = stack.pop() {
            if tree_pids.insert(pid)
                && let Some(kids) = children_map.get(&pid)
            {
                stack.extend(kids.iter().copied());
            }
        }
        for &pid in role_map.keys() {
            tree_pids.insert(pid);
        }

        let mut rust_private = 0u64;
        let mut rust_ws = 0u64;
        let mut total_private = 0u64;
        let mut total_ws = 0u64;
        let mut counted = 0usize;
        let mut renderer_count = 0usize;
        let mut current_cpu_map: HashMap<u32, u64> = HashMap::with_capacity(tree_pids.len());
        let mut role_buckets: HashMap<&'static str, (usize, u64, u64)> = HashMap::new();

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

                let role: &'static str = if pid == my_pid {
                    "Rust Host"
                } else if let Some(&known) = role_map.get(&pid) {
                    known
                } else if role_map.is_empty() && direct_children.contains(&pid) {
                    "Browser"
                } else {
                    "Crashpad / Aux"
                };

                if role == "Renderer" {
                    renderer_count += 1;
                }

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
                    let entry = role_buckets.entry(role).or_insert((0, 0, 0));
                    entry.0 += 1;
                    entry.1 = entry.1.saturating_add(priv_b);
                    entry.2 = entry.2.saturating_add(ws_b);
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
                    current_cpu_map.insert(pid, kt.saturating_add(ut));
                }

                let _ = CloseHandle(handle);
            }
        }

        let now = Instant::now();
        let cpu_percent = if let Some(prev_time) = self.last_sample_time {
            let elapsed_secs = now.duration_since(prev_time).as_secs_f64();
            if elapsed_secs > 0.1 {
                // Per-PID non-negative delta avoids negative spikes when a short-lived utility exits.
                let mut delta_100ns = 0u64;
                for (&pid, &cur_cpu) in &current_cpu_map {
                    if let Some(&prev_cpu) = self.last_pid_cpu_100ns.get(&pid) {
                        delta_100ns = delta_100ns.saturating_add(cur_cpu.saturating_sub(prev_cpu));
                    }
                }
                let delta_secs = delta_100ns as f64 / 10_000_000.0;
                let logical_cpus = std::thread::available_parallelism()
                    .map(|n| n.get() as f64)
                    .unwrap_or(1.0);
                let pct = ((delta_secs / (elapsed_secs * logical_cpus)) * 100.0).clamp(0.0, 100.0);
                if self.cpu_history.len() == ROLLING_WINDOW_SAMPLES {
                    self.cpu_history.pop_front();
                }
                self.cpu_history.push_back(pct);
                pct
            } else {
                0.0
            }
        } else {
            0.0
        };

        self.last_sample_time = Some(now);
        self.last_pid_cpu_100ns = current_cpu_map;

        let sample_count = self.cpu_history.len();
        let cpu_rolling_avg_percent = if sample_count > 0 {
            self.cpu_history.iter().sum::<f64>() / sample_count as f64
        } else {
            cpu_percent
        };

        let mut roles: Vec<RoleMemory> = role_buckets
            .into_iter()
            .map(
                |(role, (count, private_bytes, working_set_bytes))| RoleMemory {
                    role: role.to_string(),
                    count,
                    private_bytes,
                    working_set_bytes,
                },
            )
            .collect();
        roles.sort_by_key(|r| std::cmp::Reverse(r.private_bytes));

        PerformanceMetrics {
            rust_private_bytes: rust_private,
            rust_working_set_bytes: rust_ws,
            total_private_bytes: total_private,
            total_working_set_bytes: total_ws,
            webview_process_count: counted.saturating_sub(1),
            renderer_count,
            total_process_count: counted,
            cpu_percent,
            cpu_rolling_avg_percent,
            sample_count,
            window_secs: (sample_count as u64).saturating_mul(2).max(2),
            webview_low_memory,
            webview_suspended,
            runtime_version,
            roles,
        }
    }

    /// Reads real Linux `/proc` process-tree memory (`smaps_rollup` Private_Clean + Private_Dirty
    /// and RSS) and CPU ticks (`stat`) across `gemini-desk` and WebKitGTK helper processes.
    #[cfg(target_os = "linux")]
    fn sample_linux(
        &mut self,
        webview_low_memory: bool,
        webview_suspended: bool,
        runtime_version: String,
    ) -> PerformanceMetrics {
        use std::collections::HashSet;
        use std::fs;

        let my_pid = std::process::id();
        let mut tree_pids: HashSet<u32> = HashSet::new();
        let mut stack = vec![my_pid];

        while let Some(pid) = stack.pop() {
            if !tree_pids.insert(pid) {
                continue;
            }
            if let Ok(tasks) = fs::read_dir(format!("/proc/{pid}/task")) {
                for task_entry in tasks.flatten() {
                    let children_path = task_entry.path().join("children");
                    if let Ok(content) = fs::read_to_string(children_path) {
                        for tok in content.split_whitespace() {
                            if let Ok(child) = tok.parse::<u32>() {
                                stack.push(child);
                            }
                        }
                    }
                }
            }
        }

        let mut rust_private = 0u64;
        let mut rust_ws = 0u64;
        let mut total_private = 0u64;
        let mut total_ws = 0u64;
        let mut counted = 0usize;
        let mut renderer_count = 0usize;
        let mut current_cpu_map: HashMap<u32, u64> = HashMap::with_capacity(tree_pids.len());
        let mut role_buckets: HashMap<&'static str, (usize, u64, u64)> = HashMap::new();

        for &pid in &tree_pids {
            let comm = fs::read_to_string(format!("/proc/{pid}/comm"))
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            let role: &'static str = if pid == my_pid {
                "Rust Host"
            } else if comm.contains("WebKitWeb") {
                "Renderer"
            } else if comm.contains("WebKitGPU") {
                "GPU Process"
            } else if comm.contains("WebKitNetwork") {
                "Utility / Net"
            } else {
                "WebKit Aux"
            };
            if role == "Renderer" {
                renderer_count += 1;
            }

            let (priv_b, rss_b) = read_linux_proc_mem(pid);
            if priv_b > 0 || rss_b > 0 || pid == my_pid {
                let effective_priv = if priv_b > 0 { priv_b } else { rss_b };
                total_private = total_private.saturating_add(effective_priv);
                total_ws = total_ws.saturating_add(rss_b);
                if pid == my_pid {
                    rust_private = effective_priv;
                    rust_ws = rss_b;
                }
                let entry = role_buckets.entry(role).or_insert((0, 0, 0));
                entry.0 += 1;
                entry.1 = entry.1.saturating_add(effective_priv);
                entry.2 = entry.2.saturating_add(rss_b);
                counted += 1;
            }

            if let Some(cpu_100ns) = read_linux_proc_cpu_100ns(pid) {
                current_cpu_map.insert(pid, cpu_100ns);
            }
        }

        self.finalize_metrics(
            rust_private,
            rust_ws,
            total_private,
            total_ws,
            counted,
            renderer_count,
            current_cpu_map,
            role_buckets,
            webview_low_memory,
            webview_suspended,
            runtime_version,
        )
    }

    /// Reads real macOS process-tree RSS and CPU time on demand via `ps` across `gemini-desk`
    /// and its WebKit XPC helpers (`WebContent`, `GPU`, `Networking`).
    #[cfg(target_os = "macos")]
    fn sample_macos(
        &mut self,
        webview_low_memory: bool,
        webview_suspended: bool,
        runtime_version: String,
    ) -> PerformanceMetrics {
        use std::collections::HashSet;
        use std::process::Command;

        let my_pid = std::process::id();
        let mut children_map: HashMap<u32, Vec<u32>> = HashMap::new();
        let mut proc_info: HashMap<u32, (u64, u64, String)> = HashMap::new();

        if let Ok(output) = Command::new("ps")
            .args(["-eo", "pid=,ppid=,rss=,time=,comm="])
            .output()
            && output.status.success()
            && let Ok(text) = String::from_utf8(output.stdout)
        {
            for line in text.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() < 5 {
                    continue;
                }
                let Ok(pid) = parts[0].parse::<u32>() else {
                    continue;
                };
                let Ok(ppid) = parts[1].parse::<u32>() else {
                    continue;
                };
                let rss_kb = parts[2].parse::<u64>().unwrap_or(0);
                let cpu_100ns = parse_ps_time_100ns(parts[3]);
                let comm = parts[4..].join(" ");
                children_map.entry(ppid).or_default().push(pid);
                proc_info.insert(pid, (rss_kb.saturating_mul(1024), cpu_100ns, comm));
            }
        }

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
        let mut counted = 0usize;
        let mut renderer_count = 0usize;
        let mut current_cpu_map: HashMap<u32, u64> = HashMap::with_capacity(tree_pids.len());
        let mut role_buckets: HashMap<&'static str, (usize, u64, u64)> = HashMap::new();

        for &pid in &tree_pids {
            let Some((rss_b, cpu_100ns, comm)) = proc_info.get(&pid) else {
                continue;
            };
            let role: &'static str = if pid == my_pid {
                "Rust Host"
            } else if comm.contains("WebContent") {
                "Renderer"
            } else if comm.contains("WebKit.GPU") {
                "GPU Process"
            } else if comm.contains("WebKit.Networking") {
                "Utility / Net"
            } else {
                "WebKit Aux"
            };
            if role == "Renderer" {
                renderer_count += 1;
            }
            total_private = total_private.saturating_add(*rss_b);
            total_ws = total_ws.saturating_add(*rss_b);
            if pid == my_pid {
                rust_private = *rss_b;
                rust_ws = *rss_b;
            }
            let entry = role_buckets.entry(role).or_insert((0, 0, 0));
            entry.0 += 1;
            entry.1 = entry.1.saturating_add(*rss_b);
            entry.2 = entry.2.saturating_add(*rss_b);
            counted += 1;
            current_cpu_map.insert(pid, *cpu_100ns);
        }

        self.finalize_metrics(
            rust_private,
            rust_ws,
            total_private,
            total_ws,
            counted,
            renderer_count,
            current_cpu_map,
            role_buckets,
            webview_low_memory,
            webview_suspended,
            runtime_version,
        )
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[allow(clippy::too_many_arguments)]
    fn finalize_metrics(
        &mut self,
        rust_private: u64,
        rust_ws: u64,
        total_private: u64,
        total_ws: u64,
        counted: usize,
        renderer_count: usize,
        current_cpu_map: HashMap<u32, u64>,
        role_buckets: HashMap<&'static str, (usize, u64, u64)>,
        webview_low_memory: bool,
        webview_suspended: bool,
        runtime_version: String,
    ) -> PerformanceMetrics {
        let now = Instant::now();
        let logical_cores = std::thread::available_parallelism()
            .map(|n| n.get() as f64)
            .unwrap_or(1.0);

        let cpu_percent = if let Some(prev_time) = self.last_sample_time {
            let elapsed_secs = now.duration_since(prev_time).as_secs_f64();
            if elapsed_secs > 0.05 {
                let mut delta_100ns = 0u64;
                for (&pid, &curr) in &current_cpu_map {
                    if let Some(&prev) = self.last_pid_cpu_100ns.get(&pid) {
                        delta_100ns = delta_100ns.saturating_add(curr.saturating_sub(prev));
                    }
                }
                let cpu_secs = delta_100ns as f64 / 10_000_000.0;
                let pct = ((cpu_secs / (elapsed_secs * logical_cores)) * 100.0).clamp(0.0, 100.0);
                if self.cpu_history.len() >= ROLLING_WINDOW_SAMPLES {
                    self.cpu_history.pop_front();
                }
                self.cpu_history.push_back(pct);
                pct
            } else {
                0.0
            }
        } else {
            0.0
        };

        self.last_sample_time = Some(now);
        self.last_pid_cpu_100ns = current_cpu_map;

        let sample_count = self.cpu_history.len();
        let cpu_rolling_avg_percent = if sample_count > 0 {
            self.cpu_history.iter().sum::<f64>() / sample_count as f64
        } else {
            cpu_percent
        };

        let mut roles: Vec<RoleMemory> = role_buckets
            .into_iter()
            .map(
                |(role, (count, private_bytes, working_set_bytes))| RoleMemory {
                    role: role.to_string(),
                    count,
                    private_bytes,
                    working_set_bytes,
                },
            )
            .collect();
        roles.sort_by_key(|r| std::cmp::Reverse(r.private_bytes));

        PerformanceMetrics {
            rust_private_bytes: rust_private,
            rust_working_set_bytes: rust_ws,
            total_private_bytes: total_private,
            total_working_set_bytes: total_ws,
            webview_process_count: counted.saturating_sub(1),
            renderer_count,
            total_process_count: counted,
            cpu_percent,
            cpu_rolling_avg_percent,
            sample_count,
            window_secs: (sample_count as u64).saturating_mul(2).max(2),
            webview_low_memory,
            webview_suspended,
            runtime_version,
            roles,
        }
    }
}

#[cfg(target_os = "linux")]
fn read_linux_proc_mem(pid: u32) -> (u64, u64) {
    use std::fs;
    let mut priv_kb = 0u64;
    let mut rss_kb = 0u64;

    if let Ok(rollup) = fs::read_to_string(format!("/proc/{pid}/smaps_rollup")) {
        for line in rollup.lines() {
            if let Some(rest) = line.strip_prefix("Rss:") {
                rss_kb = parse_kb_field(rest);
            } else if let Some(rest) = line.strip_prefix("Private_Clean:") {
                priv_kb = priv_kb.saturating_add(parse_kb_field(rest));
            } else if let Some(rest) = line.strip_prefix("Private_Dirty:") {
                priv_kb = priv_kb.saturating_add(parse_kb_field(rest));
            }
        }
        return (priv_kb.saturating_mul(1024), rss_kb.saturating_mul(1024));
    }

    if let Ok(status) = fs::read_to_string(format!("/proc/{pid}/status")) {
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:") {
                rss_kb = parse_kb_field(rest);
            } else if let Some(rest) = line.strip_prefix("RssAnon:") {
                priv_kb = parse_kb_field(rest);
            }
        }
    }
    (priv_kb.saturating_mul(1024), rss_kb.saturating_mul(1024))
}

#[cfg(target_os = "linux")]
fn parse_kb_field(s: &str) -> u64 {
    s.split_whitespace()
        .next()
        .and_then(|n| n.parse::<u64>().ok())
        .unwrap_or(0)
}

#[cfg(target_os = "linux")]
fn read_linux_proc_cpu_100ns(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after_comm = stat.rsplit_once(')')?.1;
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    // After `comm)`, field 0 is state (field 3), so utime (14) = index 11, stime (15) = index 12.
    let utime = fields.get(11)?.parse::<u64>().ok()?;
    let stime = fields.get(12)?.parse::<u64>().ok()?;
    // Linux USER_HZ is 100 ticks/sec on x86_64/aarch64 -> 1 tick = 100,000 * 100ns.
    Some(utime.saturating_add(stime).saturating_mul(100_000))
}

#[cfg(target_os = "macos")]
fn parse_ps_time_100ns(s: &str) -> u64 {
    // Format from macOS `ps`: `MM:SS.cc` or `HH:MM:SS.cc`
    let parts: Vec<&str> = s.split(':').collect();
    let total_secs: f64 = match parts.as_slice() {
        [m, sec] => {
            let mins = m.parse::<f64>().unwrap_or(0.0);
            let secs = sec.parse::<f64>().unwrap_or(0.0);
            mins * 60.0 + secs
        }
        [h, m, sec] => {
            let hrs = h.parse::<f64>().unwrap_or(0.0);
            let mins = m.parse::<f64>().unwrap_or(0.0);
            let secs = sec.parse::<f64>().unwrap_or(0.0);
            hrs * 3600.0 + mins * 60.0 + secs
        }
        _ => 0.0,
    };
    (total_secs * 10_000_000.0).max(0.0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampler_measures_current_process_and_role_breakdown() {
        let mut sampler = PerformanceSampler::new();
        let empty = HashMap::new();
        let m = sampler.sample(false, false, "test".into(), &empty);
        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
        {
            assert!(m.total_process_count >= 1);
            assert!(m.rust_private_bytes > 0);
            assert!(m.total_private_bytes >= m.rust_private_bytes);
            assert!(m.roles.iter().any(|r| r.role == "Rust Host"));
        }
    }
}
