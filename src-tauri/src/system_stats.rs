use serde::Serialize;
use std::sync::Mutex;
use std::time::Instant;

#[derive(Serialize, Clone)]
pub struct SystemStats {
    pub cpu_pct:    f32,
    pub ram_used_gb: f32,
    pub ram_total_gb: f32,
    pub gpu_pct:    Option<f32>,
    pub gpu_vram_used_gb: Option<f32>,
    pub gpu_vram_total_gb: Option<f32>,
    pub gpu_name:   Option<String>,
    pub net_rx_mbps: f32,
    pub net_tx_mbps: f32,
}

struct NetSample {
    rx_bytes: u64,
    tx_bytes: u64,
    at: Instant,
}

static NET_PREV: Mutex<Option<NetSample>> = Mutex::new(None);

pub fn get() -> SystemStats {
    use sysinfo::{Networks, System};

    let mut sys = System::new();
    sys.refresh_cpu_usage();
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
    sys.refresh_cpu_usage();
    sys.refresh_memory();

    let cpu_pct = sys.global_cpu_usage();
    let ram_used_gb  = sys.used_memory()  as f32 / 1_073_741_824.0;
    let ram_total_gb = sys.total_memory() as f32 / 1_073_741_824.0;

    // Network throughput
    let mut networks = Networks::new_with_refreshed_list();
    networks.refresh();
    let (rx_now, tx_now): (u64, u64) = networks.iter().fold((0, 0), |acc, (_, d)| {
        (acc.0 + d.total_received(), acc.1 + d.total_transmitted())
    });
    let now = Instant::now();
    let (net_rx_mbps, net_tx_mbps) = {
        let mut prev = NET_PREV.lock().unwrap();
        let (rx_mbs, tx_mbs) = if let Some(p) = prev.as_ref() {
            let secs = now.duration_since(p.at).as_secs_f32().max(0.001);
            let rx = (rx_now.saturating_sub(p.rx_bytes)) as f32 / 1_048_576.0 / secs;
            let tx = (tx_now.saturating_sub(p.tx_bytes)) as f32 / 1_048_576.0 / secs;
            (rx, tx)
        } else {
            (0.0, 0.0)
        };
        *prev = Some(NetSample { rx_bytes: rx_now, tx_bytes: tx_now, at: now });
        (rx_mbs, tx_mbs)
    };

    // GPU via nvidia-smi
    let gpu = nvidia_smi_stats();

    SystemStats {
        cpu_pct,
        ram_used_gb,
        ram_total_gb,
        gpu_pct:           gpu.as_ref().map(|g| g.0),
        gpu_vram_used_gb:  gpu.as_ref().map(|g| g.1),
        gpu_vram_total_gb: gpu.as_ref().map(|g| g.2),
        gpu_name:          gpu.map(|g| g.3),
        net_rx_mbps,
        net_tx_mbps,
    }
}

// Returns (gpu_pct, vram_used_gb, vram_total_gb, name) or None if unavailable.
#[cfg(target_os = "windows")]
fn nvidia_smi_stats() -> Option<(f32, f32, f32, String)> {
    let mut cmd = std::process::Command::new("nvidia-smi");
    cmd.args(["--query-gpu=utilization.gpu,memory.used,memory.total,name",
              "--format=csv,noheader,nounits"]);
    crate::process_utils::no_window(&mut cmd);
    let out = cmd.output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let line = s.lines().next()?;
    let parts: Vec<&str> = line.split(',').map(str::trim).collect();
    if parts.len() < 4 { return None; }
    let pct   = parts[0].parse::<f32>().ok()?;
    let used  = parts[1].parse::<f32>().ok()? / 1024.0; // MiB → GiB
    let total = parts[2].parse::<f32>().ok()? / 1024.0;
    let name  = parts[3].to_string();
    Some((pct, used, total, name))
}

// TODO(mac): GPU stats not implemented on non-Windows platforms (returns None → dashboard omits GPU card).
#[cfg(not(target_os = "windows"))]
fn nvidia_smi_stats() -> Option<(f32, f32, f32, String)> {
    None
}
