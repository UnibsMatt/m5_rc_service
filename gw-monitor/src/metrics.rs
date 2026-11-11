//! Gateway metrics read straight from /proc and statvfs (Linux only, no heavy deps).

use std::ffi::CString;
use std::fs;
use std::net::IpAddr;
use std::time::Duration;

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Mem {
    pub total_kb: u64,
    pub available_kb: u64,
    pub used_kb: u64,
    pub pct: f64,
}

#[derive(Debug, Serialize)]
pub struct Disk {
    pub mount: String,
    pub total_kb: u64,
    pub avail_kb: u64,
    pub used_kb: u64,
    pub pct: f64,
}

#[derive(Debug, Serialize)]
pub struct Ip {
    pub iface: String,
    pub ip: IpAddr,
}

/// Keeps the previous /proc/stat sample so CPU% is a delta over the report interval.
pub struct Collector {
    prev_cpu: Option<(u64, u64)>, // (total, idle)
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

impl Collector {
    pub fn new() -> Self {
        Collector { prev_cpu: None }
    }

    /// CPU usage % since the previous call. The first call takes a short 500 ms sample.
    pub fn cpu_pct(&mut self) -> Option<f64> {
        if self.prev_cpu.is_none() {
            self.prev_cpu = read_cpu();
            std::thread::sleep(Duration::from_millis(500));
        }
        let now = read_cpu()?;
        let (pt, pi) = self.prev_cpu.replace(now)?;
        let dt = now.0.saturating_sub(pt);
        let di = now.1.saturating_sub(pi);
        if dt == 0 {
            return Some(0.0);
        }
        Some(round1((dt - di.min(dt)) as f64 * 100.0 / dt as f64))
    }
}

/// (total, idle+iowait) jiffies from the aggregate "cpu" line of /proc/stat.
fn read_cpu() -> Option<(u64, u64)> {
    let stat = fs::read_to_string("/proc/stat").ok()?;
    let line = stat.lines().find(|l| l.starts_with("cpu "))?;
    let v: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|x| x.parse().ok())
        .collect();
    if v.len() < 4 {
        return None;
    }
    // user nice system idle iowait irq softirq steal (guest* already counted in user/nice)
    let total: u64 = v.iter().take(8).sum();
    let idle = v[3] + v.get(4).copied().unwrap_or(0);
    Some((total, idle))
}

pub fn mem() -> Option<Mem> {
    let info = fs::read_to_string("/proc/meminfo").ok()?;
    let field = |name: &str| -> Option<u64> {
        info.lines()
            .find(|l| l.starts_with(name))?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    };
    let total = field("MemTotal:")?;
    let avail = field("MemAvailable:").or_else(|| {
        Some(field("MemFree:")? + field("Buffers:").unwrap_or(0) + field("Cached:").unwrap_or(0))
    })?;
    let used = total.saturating_sub(avail);
    Some(Mem {
        total_kb: total,
        available_kb: avail,
        used_kb: used,
        pct: if total > 0 { round1(used as f64 * 100.0 / total as f64) } else { 0.0 },
    })
}

/// df-style usage for each requested mount point; missing mounts are skipped.
pub fn disks(mounts: &[String]) -> Vec<Disk> {
    mounts
        .iter()
        .filter_map(|m| {
            let path = CString::new(m.as_str()).ok()?;
            let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
            if unsafe { libc::statvfs(path.as_ptr(), &mut st) } != 0 {
                return None;
            }
            let bs = st.f_frsize as u64;
            let total = st.f_blocks as u64 * bs / 1024;
            let free = st.f_bfree as u64 * bs / 1024;
            let avail = st.f_bavail as u64 * bs / 1024;
            let used = total.saturating_sub(free);
            let denom = used + avail;
            Some(Disk {
                mount: m.clone(),
                total_kb: total,
                avail_kb: avail,
                used_kb: used,
                pct: if denom > 0 { round1(used as f64 * 100.0 / denom as f64) } else { 0.0 },
            })
        })
        .collect()
}

/// Non-loopback, non-link-local addresses, optionally limited to the listed interfaces.
pub fn ips(only: &[String]) -> Vec<Ip> {
    let mut out: Vec<Ip> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback())
        .filter(|i| only.is_empty() || only.iter().any(|n| *n == i.name))
        .filter(|i| match i.ip() {
            IpAddr::V4(a) => !a.is_link_local(),
            IpAddr::V6(a) => (a.segments()[0] & 0xffc0) != 0xfe80,
        })
        .map(|i| Ip { ip: i.ip(), iface: i.name })
        .collect();
    out.sort_by(|a, b| (a.ip.is_ipv6(), &a.iface).cmp(&(b.ip.is_ipv6(), &b.iface)));
    out
}

pub fn load_avg() -> [f64; 3] {
    let s = fs::read_to_string("/proc/loadavg").unwrap_or_default();
    let mut it = s.split_whitespace().map(|x| x.parse().unwrap_or(0.0));
    [
        it.next().unwrap_or(0.0),
        it.next().unwrap_or(0.0),
        it.next().unwrap_or(0.0),
    ]
}

pub fn uptime_s() -> u64 {
    fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok())
        .map(|f| f as u64)
        .unwrap_or(0)
}

pub fn hostname() -> String {
    fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".into())
}
