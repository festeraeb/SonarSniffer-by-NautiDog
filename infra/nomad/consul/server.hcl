datacenter = "dc1"
data_dir   = "/var/lib/consul"
log_level  = "INFO"
server           = true
bootstrap_expect = 1
bind_addr        = "10.0.0.61"
client_addr      = "0.0.0.0"
advertise_addr   = "10.0.0.61"

ui_config {
  enabled = true
}

connect {
  enabled = true
}

acl {
  enabled = false
}

# Single-server bootstrap on T440 LAN; clients use retry_join in client.hcl.
