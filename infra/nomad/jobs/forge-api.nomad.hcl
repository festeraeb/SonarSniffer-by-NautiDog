# DEPRECATED on T440 — primary Forge is systemd on cesarops2 :9100.
# This job registers Consul service metadata only; it does NOT bind :9100.
# To remove T440 duplicate: bash scripts/t440-disable-deprecated-forge.sh on T440.

job "forge-api" {
  datacenters = ["dc1"]
  type        = "service"

  group "forge-meta" {
    count = 0

    constraint {
      attribute = "${meta.node.class}"
      value     = "t440-p100"
    }

    task "placeholder" {
      driver = "raw_exec"

      config {
        command = "/bin/true"
      }

      resources {
        cpu    = 10
        memory = 16
      }
    }
  }
}
