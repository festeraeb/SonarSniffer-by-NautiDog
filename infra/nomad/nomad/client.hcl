data_dir  = "/var/lib/nomad"
bind_addr = "0.0.0.0"
region    = "global"
datacenter = "dc1"
log_level = "INFO"

client {
  enabled = true
  # Set these per-node before rollout.
  node_class = "worker"
  meta {
    "node.class" = "worker"
    "gpu.class"  = "rtx2060"
    "zone"       = "lan"
  }
}

consul {
  address = "127.0.0.1:8500"
  auto_advertise = true
}

plugin "raw_exec" {
  config {
    enabled = true
  }
}

server {
  enabled = false
}
