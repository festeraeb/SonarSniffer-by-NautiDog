# One-shot: refresh /etc/exports (LAN + Tailscale). Dispatch from cesarops2:
#   NOMAD_ADDR=http://10.0.0.61:4646 nomad job run infra/nomad/jobs/t440-nfs-exports.nomad.hcl
job "t440-nfs-exports" {
  datacenters = ["dc1"]
  type        = "batch"

  group "exports" {
    count = 1

    constraint {
      attribute = "${meta.node.class}"
      value     = "t440-p100"
    }

    restart {
      attempts = 0
      mode     = "fail"
    }

    task "setup" {
      driver = "raw_exec"

      config {
        command = "/bin/bash"
        args = [
          "/codebase/repos/wreckhunter2000-1/scripts/setup_t440_nfs_exports.sh",
        ]
      }

      env {
        CESAROPS_TS_SUBNET = "100.64.0.0/10"
      }

      resources {
        cpu    = 100
        memory = 128
      }
    }
  }
}
