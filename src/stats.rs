//! The readings, each taken once per update from files or a local socket. Anything that
//! cannot be read is `None` and its place on the screen says so; nothing here spawns a process.

use std::fs;
use std::io::{Read, Write};
use std::time::Duration;

#[derive(Default, Clone)]
pub struct Gpu {
    pub name: String,
    pub load: u32,
    pub mem_used: u64,
    pub mem_total: u64,
    pub temp: u32,
    pub watts: f32,
}

#[derive(Default, Clone)]
pub struct Stats {
    pub hostname: String,
    pub uptime_s: u64,
    pub cpu_load: Option<f32>,
    pub cpu_temp: Option<f32>,
    pub loadavg: String,
    pub mem_used: u64,
    pub mem_total: u64,
    pub disk_used: u64,
    pub disk_total: u64,
    pub gpu: Option<Gpu>,
    /// `None`: Ollama did not answer. `Some(vec![])`: it runs, no model loaded.
    pub ollama: Option<Vec<(String, u64)>>,
    /// (name, running)
    pub containers: Option<Vec<(String, bool)>>,
}

pub struct Reader {
    prev_cpu: Option<(u64, u64)>,
    temp_path: Option<String>,
    nvml: Option<nvml_wrapper::Nvml>,
}

impl Reader {
    pub fn new() -> Reader {
        let nvml = nvml_wrapper::Nvml::init()
            .or_else(|_| {
                nvml_wrapper::Nvml::builder()
                    .lib_path(std::ffi::OsStr::new("libnvidia-ml.so.1"))
                    .init()
            })
            .ok();
        Reader { prev_cpu: cpu_times(), temp_path: find_cpu_temp(), nvml }
    }

    pub fn read(&mut self) -> Stats {
        let mut s = Stats {
            hostname: fs::read_to_string("/proc/sys/kernel/hostname")
                .map(|h| h.trim().to_string())
                .unwrap_or_default(),
            ..Default::default()
        };
        s.uptime_s = first_number("/proc/uptime").unwrap_or(0.0) as u64;
        s.loadavg = fs::read_to_string("/proc/loadavg")
            .map(|l| l.split_whitespace().take(3).collect::<Vec<_>>().join("  "))
            .unwrap_or_default();

        let now = cpu_times();
        if let (Some((busy0, total0)), Some((busy1, total1))) = (self.prev_cpu, now) {
            if total1 > total0 {
                s.cpu_load = Some((busy1 - busy0) as f32 * 100.0 / (total1 - total0) as f32);
            }
        }
        self.prev_cpu = now;
        s.cpu_temp = self
            .temp_path
            .as_ref()
            .and_then(|p| first_number(p))
            .map(|milli| milli as f32 / 1000.0);

        if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
            let kb = |key: &str| {
                meminfo
                    .lines()
                    .find(|l| l.starts_with(key))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0)
                    * 1024
            };
            s.mem_total = kb("MemTotal:");
            s.mem_used = s.mem_total.saturating_sub(kb("MemAvailable:"));
        }

        let mut vfs: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(c"/".as_ptr(), &mut vfs) } == 0 {
            let block = vfs.f_frsize as u64;
            s.disk_total = vfs.f_blocks as u64 * block;
            s.disk_used = s.disk_total.saturating_sub(vfs.f_bfree as u64 * block);
        }

        s.gpu = self.nvml.as_ref().and_then(read_gpu);
        s.ollama = read_ollama();
        s.containers = read_containers();
        s
    }
}

fn first_number(path: &str) -> Option<f64> {
    fs::read_to_string(path).ok()?.split_whitespace().next()?.parse().ok()
}

/// (busy, total) jiffies over all cores.
fn cpu_times() -> Option<(u64, u64)> {
    let stat = fs::read_to_string("/proc/stat").ok()?;
    let v: Vec<u64> = stat
        .lines()
        .next()?
        .split_whitespace()
        .skip(1)
        .filter_map(|x| x.parse().ok())
        .collect();
    if v.len() < 8 {
        return None;
    }
    let idle = v[3] + v[4];
    let total: u64 = v[..8].iter().sum();
    Some((total - idle, total))
}

fn find_cpu_temp() -> Option<String> {
    for entry in fs::read_dir("/sys/class/hwmon").ok()?.flatten() {
        let dir = entry.path();
        let name = fs::read_to_string(dir.join("name")).unwrap_or_default();
        if matches!(name.trim(), "k10temp" | "coretemp" | "zenpower") {
            return Some(dir.join("temp1_input").to_string_lossy().into_owned());
        }
    }
    None
}

fn read_gpu(nvml: &nvml_wrapper::Nvml) -> Option<Gpu> {
    use nvml_wrapper::enum_wrappers::device::TemperatureSensor;
    let dev = nvml.device_by_index(0).ok()?;
    let mem = dev.memory_info().ok()?;
    Some(Gpu {
        name: dev
            .name()
            .unwrap_or_default()
            .replace("NVIDIA ", "")
            .replace("GeForce ", ""),
        load: dev.utilization_rates().map(|u| u.gpu).unwrap_or(0),
        mem_used: mem.used,
        mem_total: mem.total,
        temp: dev.temperature(TemperatureSensor::Gpu).unwrap_or(0),
        watts: dev.power_usage().map(|mw| mw as f32 / 1000.0).unwrap_or(0.0),
    })
}

/// The body of a plain HTTP/1.0 GET; the server closes the connection when it is done.
fn http_body<S: Read + Write>(mut stream: S, path: &str) -> Option<serde_json::Value> {
    write!(stream, "GET {path} HTTP/1.0\r\nHost: localhost\r\n\r\n").ok()?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).ok()?;
    let at = raw.windows(4).position(|w| w == b"\r\n\r\n")? + 4;
    serde_json::from_slice(&raw[at..]).ok()
}

fn read_ollama() -> Option<Vec<(String, u64)>> {
    let addr = "127.0.0.1:11434".parse().ok()?;
    let stream = std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(300)).ok()?;
    stream.set_read_timeout(Some(Duration::from_millis(1000))).ok()?;
    let json = http_body(stream, "/api/ps")?;
    Some(
        json.get("models")?
            .as_array()?
            .iter()
            .map(|m| {
                (
                    m["name"].as_str().unwrap_or("?").to_string(),
                    m["size_vram"].as_u64().unwrap_or(0),
                )
            })
            .collect(),
    )
}

fn read_containers() -> Option<Vec<(String, bool)>> {
    let stream = std::os::unix::net::UnixStream::connect("/var/run/docker.sock").ok()?;
    stream.set_read_timeout(Some(Duration::from_millis(1500))).ok()?;
    let json = http_body(stream, "/containers/json?all=true")?;
    let mut list: Vec<(String, bool)> = json
        .as_array()?
        .iter()
        .map(|c| {
            (
                c["Names"][0].as_str().unwrap_or("?").trim_start_matches('/').to_string(),
                c["State"].as_str() == Some("running"),
            )
        })
        .collect();
    // stopped ones first: they are what the screen is for
    list.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    Some(list)
}
