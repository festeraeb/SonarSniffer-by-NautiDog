use std::fs;
use tracing::{info, warn};
use crate::types::Error;

/// Represents the NUMA topology of the system
pub struct NumaTopology {
    pub socket_count: usize,
    pub gpu_affinity: Vec<(u32, u32)>, // (gpu_index, socket_id)
    pub cores_per_socket: Vec<Vec<u32>>,
}

impl NumaTopology {
    /// Detects NUMA topology by reading /sys filesystem
    pub fn detect() -> Result<Self, Error> {
        let sys_node_path = "/sys/devices/system/node";

        // 1. Count NUMA nodes
        let node_count = match fs::read_dir(sys_node_path) {
            Ok(entries) => entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .starts_with("node")
                })
                .count(),
            Err(_) => {
                warn!("Cannot read /sys/devices/system/node — assuming single NUMA node");
                1
            }
        };

        if node_count > 2 {
            info!("Sub-NUMA Clustering (SNC) detected: {} nodes found", node_count);
        }

        info!("Detected {} NUMA nodes", node_count);

        // 2. Discover NVIDIA GPUs and map to NUMA nodes via /sys/bus/pci/devices
        let mut gpu_affinity: Vec<(u32, u32)> = Vec::new();
        let pci_path = "/sys/bus/pci/devices";

        if let Ok(entries) = fs::read_dir(pci_path) {
            for entry in entries.flatten() {
                let device_path = entry.path();
                let vendor_file = device_path.join("vendor");

                if let Ok(vendor) = fs::read_to_string(&vendor_file) {
                    if vendor.trim() == "0x10de" {
                        // NVIDIA device found — read its NUMA node
                        let numa_file = device_path.join("numa_node");
                        let socket_id = fs::read_to_string(&numa_file)
                            .ok()
                            .and_then(|s| s.trim().parse::<i32>().ok())
                            .unwrap_or(0);

                        // numa_node returns -1 if not NUMA-aware; treat as socket 0
                        let socket_id = socket_id.max(0) as u32;
                        let gpu_index = gpu_affinity.len() as u32;
                        gpu_affinity.push((gpu_index, socket_id));
                        info!("GPU {} mapped to NUMA node {}", gpu_index, socket_id);
                    }
                }
            }
        }

        // 3. Build cores_per_socket (read from /sys/devices/system/node/nodeN/cpulist)
        let mut cores_per_socket: Vec<Vec<u32>> = Vec::with_capacity(node_count);
        for i in 0..node_count {
            let cpulist_path = format!("{}/node{}/cpulist", sys_node_path, i);
            let cores = match fs::read_to_string(&cpulist_path) {
                Ok(content) => parse_cpulist(content.trim()),
                Err(_) => Vec::new(),
            };
            cores_per_socket.push(cores);
        }

        Ok(NumaTopology {
            socket_count: node_count,
            gpu_affinity,
            cores_per_socket,
        })
    }

    /// Pins the current thread to a specific socket's cores using sched_setaffinity
    pub fn pin_worker_to_socket(&self, socket_id: u32) -> Result<(), Error> {
        let cores = self.cores_per_socket
            .get(socket_id as usize)
            .ok_or_else(|| Error::NumaMismatch(
                format!("Socket {} not found (have {} sockets)", socket_id, self.socket_count)
            ))?;

        if cores.is_empty() {
            warn!("No cores found for socket {} — skipping pin", socket_id);
            return Ok(());
        }

        #[cfg(target_os = "linux")]
        {
            use std::mem;
            unsafe {
                let mut cpu_set: libc::cpu_set_t = mem::zeroed();
                for &core in cores {
                    libc::CPU_SET(core as usize, &mut cpu_set);
                }
                let ret = libc::sched_setaffinity(
                    0, // 0 = current thread
                    mem::size_of::<libc::cpu_set_t>(),
                    &cpu_set,
                );
                if ret != 0 {
                    return Err(Error::NumaMismatch(
                        format!("sched_setaffinity failed for socket {}", socket_id)
                    ));
                }
            }
            info!("Thread pinned to socket {} (cores: {:?})", socket_id, &cores[..cores.len().min(4)]);
        }

        #[cfg(not(target_os = "linux"))]
        {
            warn!("NUMA pinning not supported on this OS — skipping");
        }

        Ok(())
    }
}

/// Parse a Linux cpulist string like "0-7,16-23" into a Vec of core IDs
fn parse_cpulist(cpulist: &str) -> Vec<u32> {
    let mut cores = Vec::new();
    for part in cpulist.split(',') {
        let part = part.trim();
        if let Some((start, end)) = part.split_once('-') {
            if let (Ok(s), Ok(e)) = (start.parse::<u32>(), end.parse::<u32>()) {
                for i in s..=e {
                    cores.push(i);
                }
            }
        } else if let Ok(n) = part.parse::<u32>() {
            cores.push(n);
        }
    }
    cores
}
