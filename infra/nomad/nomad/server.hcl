data_dir  = "/var/lib/nomad"
bind_addr = "0.0.0.0"
region    = "global"
datacenter = "dc1"
log_level = "INFO"

advertise {
  http = "10.0.0.61"
  rpc  = "10.0.0.61"
  serf = "10.0.0.61"
}

server {
  enabled          = true
  bootstrap_expect = 1
}

client {
  enabled = true
  node_class = "t440-p100"
  meta {
    "node.class" = "t440-p100"
    "gpu.class"  = "p100"
    "zone"       = "lan"
  }
}

consul {
  address = "127.0.0.1:8500"
  auto_advertise = true
  server_service_name = "nomad"
  client_service_name = "nomad-client"
}
