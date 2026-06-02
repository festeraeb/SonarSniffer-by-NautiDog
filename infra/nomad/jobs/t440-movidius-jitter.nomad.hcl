# Lock-3 jitter on T440 (:8180) — jitter-rs (Rust), not legacy Python jitter_movidius.py.
# Movidius NCS2 is a validator vote; Coral on ML350e (:8190) is optional remote validator.
# Run from cesarops2: NOMAD_ADDR=http://10.0.0.61:4646 nomad job run infra/nomad/jobs/t440-movidius-jitter.nomad.hcl
job "t440-movidius-jitter" {
  datacenters = ["dc1"]
  type        = "service"

  group "jitter" {
    count = 1

    constraint {
      attribute = "${meta.node.class}"
      value     = "t440-p100"
    }

    network {
      port "jitter" {
        static = 8180
      }
    }

    task "jitter-rs" {
      driver = "raw_exec"

      config {
        command = "/bin/bash"
        args = [
          "/codebase/repos/wreckhunter2000-1/scripts/t440-start-jitter-rs.sh",
        ]
      }

      env {
        HOME                  = "/home/cesarops"
        JITTER_PORT           = "8180"
        REPO                  = "/codebase/repos/wreckhunter2000-1"
        ML350E_LAN            = "10.0.0.201"
        JITTER_REMOTE_VALIDATORS = "coral_edgetpu=http://10.0.0.201:8190"
        JITTER_RS_SKIP_BUILD     = "1"
        JITTER_RS_BIN            = "/codebase/repos/wreckhunter2000-1/cesarops-detection/jitter-rs/target/release/jitter-rs"
        RUST_LOG                 = "info"
      }

      resources {
        cpu    = 2000
        memory = 3072
      }

      service {
        name = "t440-jitter"
        port = "jitter"
        check {
          type     = "http"
          path     = "/health"
          interval = "15s"
          timeout  = "3s"
        }
      }
    }
  }
}
