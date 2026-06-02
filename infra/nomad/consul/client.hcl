datacenter = "dc1"
data_dir   = "/var/lib/consul"
log_level  = "INFO"
server     = false
# Fixed LAN fleet (T440 + cesarops2): bootstrap_client.sh sets bind/advertise per host.
# Laptop only: pass --remote to join via Tailscale (see bootstrap_client.sh).
bind_addr      = "10.0.0.201"
client_addr    = "0.0.0.0"
advertise_addr = "10.0.0.201"

connect {
  enabled = true
}

acl {
  enabled = false
}

# T440 Consul server on LAN (not Tailscale).
retry_join = ["10.0.0.61"]
