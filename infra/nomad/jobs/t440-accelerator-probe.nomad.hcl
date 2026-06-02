job "t440-accelerator-probe" {
  datacenters = ["dc1"]
  type        = "batch"

  group "probe" {
    count = 1

    constraint {
      attribute = "${attr.unique.hostname}"
      value     = "t440cesarops"
    }

    task "probe" {
      driver = "raw_exec"

      config {
        command = "/bin/bash"
        args = [
          "-lc",
          "lsusb && lsusb -d 03e7: || true && lspci -nn | grep -i coral || true && ls /dev/apex* 2>/dev/null || true && ss -tlnp | grep -E ':8080|:8180' || true",
        ]
      }

      resources {
        cpu    = 100
        memory = 64
      }
    }
  }
}
