use std::{
    fs,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::Result;
use rustyjack_core::Commands;
use rustyjack_core::cli::StatusCommand;
use serde_json::Value;
use walkdir::WalkDir;

use crate::{core::CoreBridge, display::StatusOverlay};

pub struct StatsSampler {
    data: Arc<Mutex<StatusOverlay>>,
    stop: Arc<AtomicBool>,
}

impl StatsSampler {
    pub fn spawn(core: CoreBridge) -> Self {
        let data = Arc::new(Mutex::new(StatusOverlay::default()));
        let stop = Arc::new(AtomicBool::new(false));

        let data_clone = data.clone();
        let stop_clone = stop.clone();
        let root = core.root().to_path_buf();

        thread::spawn(move || {
            while !stop_clone.load(Ordering::Relaxed) {
                if let Err(err) = sample_once(&core, &data_clone, &root) {
                    eprintln!("[stats] sampler error: {err:?}");
                }
                thread::sleep(Duration::from_secs(2));
            }
        });

        Self { data, stop }
    }

    pub fn snapshot(&self) -> StatusOverlay {
        self.data
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }
}

impl Drop for StatsSampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn sample_once(core: &CoreBridge, shared: &Arc<Mutex<StatusOverlay>>, root: &Path) -> Result<()> {
    let temp = read_temp().unwrap_or_default();
    let (cpu_percent, uptime_secs) = read_cpu_and_uptime().unwrap_or((0.0, 0));
    let (mem_used_mb, mem_total_mb) = read_memory().unwrap_or((0, 0));
    let (disk_used_gb, disk_total_gb) = read_disk_usage(root.to_str().unwrap_or("/root/Rustyjack")).unwrap_or((0.0, 0.0));
    let (net_rx_bytes, net_tx_bytes) = read_network_total().unwrap_or((0, 0));
    let packets_captured = count_captured_packets(&root.join("loot/MITM").to_string_lossy()).unwrap_or(0);
    let creds_found = count_credentials(&root.join("loot").to_string_lossy()).unwrap_or(0);
    let mitm_victims = count_mitm_victims().unwrap_or(0);
    
    let mut overlay = {
        let guard = shared.lock().unwrap();
        let mut snapshot = guard.clone();
        
        // Calculate network rate (bytes/sec since last sample)
        let rx_delta = net_rx_bytes.saturating_sub(snapshot.net_rx_bytes);
        let tx_delta = net_tx_bytes.saturating_sub(snapshot.net_tx_bytes);
        snapshot.net_rx_rate = rx_delta as f32 / 2.0; // 2 second intervals
        snapshot.net_tx_rate = tx_delta as f32 / 2.0;
        
        snapshot.temp_c = temp;
        snapshot.cpu_percent = cpu_percent;
        snapshot.mem_used_mb = mem_used_mb;
        snapshot.mem_total_mb = mem_total_mb;
        snapshot.disk_used_gb = disk_used_gb;
        snapshot.disk_total_gb = disk_total_gb;
        snapshot.net_rx_bytes = net_rx_bytes;
        snapshot.net_tx_bytes = net_tx_bytes;
        snapshot.uptime_secs = uptime_secs;
        snapshot.packets_captured = packets_captured;
        snapshot.creds_found = creds_found;
        snapshot.mitm_victims = mitm_victims;
        snapshot
    };

    if let Ok((_, data)) = core.dispatch(Commands::Status(StatusCommand::Summary)) {
        if let Some(text) = extract_status_text(&data) {
            overlay.text = text.clone();
            overlay.active_operations = parse_active_operations(&text);
        }
    }

    // Check autopilot status
    if let Ok((_, data)) = core.dispatch(Commands::Autopilot(rustyjack_core::cli::AutopilotCommand::Status)) {
        overlay.autopilot_running = data.get("running")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        overlay.autopilot_mode = data.get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
    } else {
        overlay.autopilot_running = false;
        overlay.autopilot_mode = String::new();
    }

    if let Ok(mut guard) = shared.lock() {
        *guard = overlay;
    }
    Ok(())
}

fn extract_status_text(data: &Value) -> Option<String> {
    match data {
        Value::Object(map) => map
            .get("status_text")
            .and_then(|value| value.as_str())
            .map(|s| s.to_string()),
        _ => None,
    }
}

fn read_temp() -> Result<f32> {
    let raw = fs::read_to_string("/sys/class/thermal/thermal_zone0/temp")?;
    let value: f32 = raw.trim().parse::<u32>().unwrap_or(0) as f32 / 1000.0;
    Ok(value)
}

fn read_cpu_and_uptime() -> Result<(f32, u64)> {
    let uptime_raw = fs::read_to_string("/proc/uptime")?;
    let uptime_secs = uptime_raw
        .split_whitespace()
        .next()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0) as u64;

    let loadavg_raw = fs::read_to_string("/proc/loadavg")?;
    let load1min = loadavg_raw
        .split_whitespace()
        .next()
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(0.0);
    
    let cpu_count = num_cpus::get() as f32;
    let cpu_percent = (load1min / cpu_count * 100.0).min(100.0);
    
    Ok((cpu_percent, uptime_secs))
}

fn read_memory() -> Result<(u64, u64)> {
    let meminfo = fs::read_to_string("/proc/meminfo")?;
    let mut total = 0u64;
    let mut available = 0u64;
    
    for line in meminfo.lines() {
        if line.starts_with("MemTotal:") {
            total = line
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
        } else if line.starts_with("MemAvailable:") {
            available = line
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
        }
    }
    
    let total_mb = total / 1024;
    let used_mb = (total.saturating_sub(available)) / 1024;
    Ok((used_mb, total_mb))
}

fn read_disk_usage(path: &str) -> Result<(f32, f32)> {
    let output = std::process::Command::new("df")
        .arg("-BG")
        .arg(path)
        .output()?;
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.lines().nth(1).unwrap_or("");
    let parts: Vec<&str> = line.split_whitespace().collect();
    
    if parts.len() >= 4 {
        let total = parts[1].trim_end_matches('G').parse::<f32>().unwrap_or(0.0);
        let used = parts[2].trim_end_matches('G').parse::<f32>().unwrap_or(0.0);
        return Ok((used, total));
    }
    
    Ok((0.0, 0.0))
}

fn read_network_total() -> Result<(u64, u64)> {
    let mut rx_total = 0u64;
    let mut tx_total = 0u64;
    
    for entry in fs::read_dir("/sys/class/net")? {
        let entry = entry?;
        let ifname = entry.file_name();
        let ifname_str = ifname.to_string_lossy();
        
        if ifname_str == "lo" {
            continue;
        }
        
        let rx_path = format!("/sys/class/net/{}/statistics/rx_bytes", ifname_str);
        let tx_path = format!("/sys/class/net/{}/statistics/tx_bytes", ifname_str);
        
        if let Ok(rx_str) = fs::read_to_string(&rx_path) {
            rx_total += rx_str.trim().parse::<u64>().unwrap_or(0);
        }
        if let Ok(tx_str) = fs::read_to_string(&tx_path) {
            tx_total += tx_str.trim().parse::<u64>().unwrap_or(0);
        }
    }
    
    Ok((rx_total, tx_total))
}

fn count_captured_packets(pcap_dir: &str) -> Result<u64> {
    let mut total = 0u64;
    
    if !Path::new(pcap_dir).exists() {
        return Ok(0);
    }
    
    for entry in WalkDir::new(pcap_dir).max_depth(2) {
        let entry = entry?;
        if entry.path().extension().and_then(|s| s.to_str()) == Some("pcap") {
            if let Ok(metadata) = entry.metadata() {
                total += metadata.len() / 100;
            }
        }
    }
    
    Ok(total)
}

fn count_credentials(loot_dir: &str) -> Result<u32> {
    let mut count = 0u32;
    let responder_dir = format!("{}/Responder", loot_dir);
    
    if !Path::new(&responder_dir).exists() {
        return Ok(0);
    }
    
    for entry in WalkDir::new(&responder_dir).max_depth(2) {
        let entry = entry?;
        if entry.file_type().is_file() {
            if let Ok(content) = fs::read_to_string(entry.path()) {
                count += content.lines().filter(|l| l.contains("::")).count() as u32;
            }
        }
    }
    
    Ok(count)
}

fn count_mitm_victims() -> Result<u32> {
    let output = std::process::Command::new("arp")
        .arg("-n")
        .output()?;
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    let count = stdout.lines().skip(1).filter(|l| !l.is_empty()).count() as u32;
    Ok(count.saturating_sub(1))
}

fn parse_active_operations(status_text: &str) -> Vec<String> {
    let mut ops = Vec::new();
    let lower = status_text.to_lowercase();
    
    if lower.contains("scan") {
        ops.push("Nmap Scan".to_string());
    }
    if lower.contains("mitm") {
        ops.push("MITM Attack".to_string());
    }
    if lower.contains("responder") {
        ops.push("Responder".to_string());
    }
    if lower.contains("dns") || lower.contains("spoof") {
        ops.push("DNS Spoofing".to_string());
    }
    if lower.contains("bridge") {
        ops.push("Bridge Mode".to_string());
    }
    
    if ops.is_empty() {
        ops.push("Idle".to_string());
    }
    
    ops
}
