use anyhow::Result;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use nauticuvs::protocol::NodeCapabilities;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, UdpSocket};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

const SERVICE_TYPE: &str = "_cesarops._tcp.local.";

pub struct NodeDiscovery {
    daemon: ServiceDaemon,
    peers: Arc<RwLock<HashMap<String, NodeCapabilities>>>,
}

impl NodeDiscovery {
    pub fn new() -> Result<Self> {
        let daemon = ServiceDaemon::new()?;
        Ok(Self {
            daemon,
            peers: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub fn announce(&self, caps: &NodeCapabilities, port: u16) -> Result<()> {
        let host_ip: IpAddr = local_ipv4()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
        // mDNS hostnames must not contain dots other than the .local. suffix
        let safe_name = caps.node_id.replace('.', "-");
        let hostname = format!("{}.local.", safe_name);

        let props: HashMap<String, String> = [
            ("vram_gb".to_string(), caps.total_vram_gb.to_string()),
            ("fp64".to_string(), caps.has_fp64.to_string()),
            ("tpu".to_string(), caps.has_tpu.to_string()),
            ("gpu".to_string(), caps.gpu_name.clone()),
            ("port".to_string(), port.to_string()),
        ]
        .into_iter()
        .collect();

        let service = ServiceInfo::new(
            SERVICE_TYPE,
            &safe_name,
            &hostname,
            host_ip,
            port,
            Some(props),
        )?;

        self.daemon.register(service)?;
        info!("mDNS: announced {} ({}) on port {}", safe_name, host_ip, port);
        Ok(())
    }

    pub async fn browse_peers(&self) -> Result<()> {
        let receiver = self.daemon.browse(SERVICE_TYPE)?;
        let peers = self.peers.clone();

        info!("mDNS: starting peer discovery for service type: {}", SERVICE_TYPE);

        // Passive listener task
        tokio::spawn(async move {
            loop {
                match receiver.recv_async().await {
                    Ok(ServiceEvent::ServiceResolved(info)) => {
                        let fullname = info.get_fullname();
                        let props = info.get_properties();
                        let vram_gb = props
                            .get("vram_gb")
                            .and_then(|p| p.val_str().parse::<u32>().ok())
                            .unwrap_or(0);
                        let has_fp64 = props
                            .get("fp64")
                            .and_then(|p| p.val_str().parse::<bool>().ok())
                            .unwrap_or(false);
                        let has_tpu = props
                            .get("tpu")
                            .and_then(|p| p.val_str().parse::<bool>().ok())
                            .unwrap_or(false);
                        let gpu_name = props
                            .get("gpu")
                            .map(|p| p.val_str().to_string())
                            .unwrap_or_default();

                        let peer = NodeCapabilities {
                            node_id: fullname.to_string(),
                            total_vram_gb: vram_gb,
                            available_vram_gb: vram_gb,
                            has_fp64,
                            has_tpu,
                            gpu_name: gpu_name.clone(),
                        };

                        info!("mDNS: peer resolved: {} | GPU: {} | VRAM: {}GB | FP64: {} | TPU: {}",
                            fullname, gpu_name, vram_gb, has_fp64, has_tpu);
                        peers.write().await.insert(fullname.to_string(), peer);
                    }
                    Ok(ServiceEvent::ServiceRemoved(_, fullname)) => {
                        info!("mDNS: peer departed — {}", fullname);
                        peers.write().await.remove(&fullname);
                    }
                    Ok(other) => {
                        info!("mDNS: service event — {:?}", other);
                    }
                    Err(e) => {
                        error!("mDNS browse error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    /// Start active periodic discovery polling — queries across mDNS, Tailscale, and home DNS.
    /// Runs every 30 seconds to find nodes that may not have announced yet.
    pub fn spawn_periodic_discovery(&self) {
        let daemon = self.daemon.clone();
        let peers = self.peers.clone();
        let service_type = SERVICE_TYPE.to_string();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
            loop {
                interval.tick().await;

                // Active mDNS query: re-browse to catch new services
                if let Ok(receiver) = daemon.browse(&service_type) {
                    info!("mDNS: periodic query initiated for {}", service_type);
                    // Collect results from this query
                    let peers_clone = peers.clone();
                    tokio::spawn(async move {
                        for _ in 0..100 {
                            // Listen for up to 100 events or timeout
                            match tokio::time::timeout(
                                tokio::time::Duration::from_secs(2),
                                receiver.recv_async(),
                            )
                            .await
                            {
                                Ok(Ok(ServiceEvent::ServiceResolved(info))) => {
                                    let fullname = info.get_fullname();
                                    let props = info.get_properties();
                                    let vram_gb = props
                                        .get("vram_gb")
                                        .and_then(|p| p.val_str().parse::<u32>().ok())
                                        .unwrap_or(0);
                                    let has_fp64 = props
                                        .get("fp64")
                                        .and_then(|p| p.val_str().parse::<bool>().ok())
                                        .unwrap_or(false);
                                    let has_tpu = props
                                        .get("tpu")
                                        .and_then(|p| p.val_str().parse::<bool>().ok())
                                        .unwrap_or(false);
                                    let gpu_name = props
                                        .get("gpu")
                                        .map(|p| p.val_str().to_string())
                                        .unwrap_or_default();

                                    let peer = NodeCapabilities {
                                        node_id: fullname.to_string(),
                                        total_vram_gb: vram_gb,
                                        available_vram_gb: vram_gb,
                                        has_fp64,
                                        has_tpu,
                                        gpu_name,
                                    };

                                    peers_clone.write().await.insert(fullname.to_string(), peer);
                                }
                                Ok(Ok(ServiceEvent::ServiceRemoved(_, fullname))) => {
                                    peers_clone.write().await.remove(&fullname);
                                }
                                _ => break,
                            }
                        }
                    });
                }

                // Try to resolve common Tailscale IPs (100.x.x.x range)
                Self::query_tailscale_peers(&peers).await;
            }
        });
    }

    async fn query_tailscale_peers(peers: &Arc<RwLock<HashMap<String, NodeCapabilities>>>) {
        // Build candidate list from env vars — covers all known cluster nodes.
        // CLUSTER_NODES=ip1:port,ip2:port,... overrides defaults.
        // Falls back to the known CESARops cluster IPs from .env.
        let mut candidates: Vec<(String, u16)> = Vec::new();

        if let Ok(env_nodes) = std::env::var("CLUSTER_NODES") {
            for entry in env_nodes.split(',') {
                let entry = entry.trim();
                if entry.is_empty() { continue; }
                let (ip, port) = if let Some(pos) = entry.rfind(':') {
                    let port = entry[pos+1..].parse::<u16>().unwrap_or(8765);
                    (entry[..pos].to_string(), port)
                } else {
                    (entry.to_string(), 8765)
                };
                candidates.push((ip, port));
            }
        }

        // Always include the known cluster nodes from .env
        let known = [
            (std::env::var("I7_HOST").unwrap_or_else(|_| "10.0.0.56".into()),       8765u16),
            (std::env::var("I7_TAILSCALE").unwrap_or_else(|_| "100.85.138.4".into()), 8765),
            (std::env::var("XENON_HOST").unwrap_or_else(|_| "10.0.0.129".into()),    8765),
            (std::env::var("P1000_HOST").unwrap_or_else(|_| "10.0.0.204".into()),    8765),
            (std::env::var("PI_HOST").unwrap_or_else(|_| "10.0.0.226".into()),       8765),
            (std::env::var("PI_TAILSCALE").unwrap_or_else(|_| "100.127.66.32".into()), 8765),
            (std::env::var("P1000_TAILSCALE").unwrap_or_else(|_| "100.105.77.74".into()), 8765),
            // Xbox conductor — runs on port 8000
            (std::env::var("XBOX_HOST").unwrap_or_else(|_| "10.0.0.100".into()),     8000),
            // T440 — dual P100, primary compute node
            (std::env::var("T440_HOST").unwrap_or_else(|_| "10.0.0.61".into()),      8765),
        ];
        for (ip, port) in known {
            if !candidates.iter().any(|(i, p)| i == &ip && *p == port) {
                candidates.push((ip, port));
            }
        }

        let client = reqwest::Client::builder()
            .timeout(tokio::time::Duration::from_secs(2))
            .build()
            .unwrap_or_default();

        for (ip, port) in candidates {
            let url = format!("http://{}:{}/v1/node/status", ip, port);
            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(status) = resp.json::<serde_json::Value>().await {
                        let node_id = status["node_id"].as_str().unwrap_or(&ip).to_string();
                        let gpu_name = status["gpu_name"].as_str().unwrap_or("Unknown").to_string();
                        let total_vram = status["total_vram_gb"].as_u64().unwrap_or(0) as u32;
                        let avail_vram = status["available_vram_gb"].as_u64().unwrap_or(total_vram as u64) as u32;
                        let has_fp64 = status["has_fp64"].as_bool().unwrap_or(false);
                        let has_tpu = status["has_tpu"].as_bool().unwrap_or(false);

                        let peer = NodeCapabilities {
                            node_id: node_id.clone(),
                            total_vram_gb: total_vram,
                            available_vram_gb: avail_vram,
                            has_fp64,
                            has_tpu,
                            gpu_name: gpu_name.clone(),
                        };
                        info!("Cluster probe: {} at {}:{} | {} | {}GB VRAM", node_id, ip, port, gpu_name, total_vram);
                        peers.write().await.insert(node_id, peer);
                    }
                }
                _ => {}
            }
        }
    }

    pub async fn get_peers(&self) -> Vec<NodeCapabilities> {
        self.peers.read().await.values().cloned().collect()
    }

    /// Find the best peer for a task requiring specific resources.
    pub async fn find_peer_for(
        &self,
        required_vram_gb: u32,
        required_fp64: bool,
        requires_tpu: bool,
    ) -> Option<NodeCapabilities> {
        let peers = self.peers.read().await;
        peers
            .values()
            .filter(|p| {
                p.available_vram_gb >= required_vram_gb
                    && (!required_fp64 || p.has_fp64)
                    && (!requires_tpu || p.has_tpu)
            })
            .max_by_key(|p| p.available_vram_gb)
            .cloned()
    }
}

fn local_ipv4() -> Option<Ipv4Addr> {
    // Try the UDP trick first to get the preferred outbound IP
    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(std::net::SocketAddr::V4(addr)) = socket.local_addr() {
                let ip = *addr.ip();
                if ip != Ipv4Addr::LOCALHOST && ip != Ipv4Addr::UNSPECIFIED {
                    info!("mDNS: detected local IP {}", ip);
                    return Some(ip);
                }
            }
        }
    }

    // Fallback: try to find the first non-loopback IPv4 address on the system
    if let Ok(addrs) = hostname::get() {
        info!("mDNS: hostname is {:?}", addrs);
    }

    warn!("mDNS: could not detect local IPv4, using 0.0.0.0");
    None
}
