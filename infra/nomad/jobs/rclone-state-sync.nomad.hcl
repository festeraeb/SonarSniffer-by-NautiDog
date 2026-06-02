job "rclone-state-sync" {
  datacenters = ["dc1"]
  type        = "batch"

  periodic {
    crons            = ["*/15 * * * *"]
    prohibit_overlap = true
  }

  group "sync" {
    constraint {
      attribute = "${meta.node.class}"
      value     = "t440-p100"
    }

    task "rclone" {
      driver = "raw_exec"

      config {
        command = "/bin/bash"
        args = [
          "/codebase/repos/wreckhunter2000-1/infra/nomad/rclone/rclone-state-sync.sh",
        ]
      }

      env {
        RCLONE_CONFIG = "/home/cesarops/.config/rclone/rclone.conf"
        HOME          = "/home/cesarops"
      }

      resources {
        cpu    = 200
        memory = 256
      }
    }
  }
}
