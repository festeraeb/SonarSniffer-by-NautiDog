# One-shot: remove Forge from T440 (primary stays on cesarops2).
# Dispatch from cesarops2:
#   NOMAD_ADDR=http://10.0.0.61:4646 nomad job run infra/nomad/jobs/t440-remove-forge.nomad.hcl
job "t440-remove-forge" {
  datacenters = ["dc1"]
  type        = "batch"

  group "remove" {
    count = 1

    constraint {
      attribute = "${meta.node.class}"
      value     = "t440-p100"
    }

    restart {
      attempts = 0
      mode     = "fail"
    }

    task "remove-forge" {
      driver = "raw_exec"

      config {
        command = "/bin/bash"
        args = [
          "/codebase/repos/wreckhunter2000-1/scripts/t440-remove-forge.sh",
        ]
      }

      env {
        HOME = "/home/cesarops"
      }

      resources {
        cpu    = 100
        memory = 128
      }
    }
  }
}
