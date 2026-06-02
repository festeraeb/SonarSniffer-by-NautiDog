job "pingora-edge" {
  datacenters = ["dc1"]
  type = "service"

  group "edge" {
    count = 1

    constraint {
      attribute = "${meta.node.class}"
      value     = "t440-p100"
    }

    network {
      port "http" {
        static = 8088
      }
    }

    task "pingora" {
      driver = "raw_exec"

      env {
        RUST_LOG = "info"
      }

      config {
        command = "/codebase/repos/wreckhunter2000-1/target/release/cesarops-pingora-edge"
        args = [
          "--config",
          "${NOMAD_TASK_DIR}/pingora-routes.toml"
        ]
      }

      template {
        destination = "${NOMAD_TASK_DIR}/pingora-routes.toml"
        data = <<EOF
[server]
listen = "0.0.0.0:8088"

# PRIMARY Forge — cesarops2 (never T440 :9100)
[[routes]]
path_prefix = "/api/forge/"
upstream = "http://10.0.0.201:9100"

# dual-coder-zaya layout
[[routes]]
path_prefix = "/api/llm/coder"
upstream = "http://10.0.0.61:5001"

[[routes]]
path_prefix = "/api/llm/coder-b"
upstream = "http://10.0.0.201:5200"

[[routes]]
path_prefix = "/api/llm/reviewer"
upstream = "http://10.0.0.61:5002"

[[routes]]
path_prefix = "/api/llm/thinker"
upstream = "http://10.0.0.201:5203"
EOF
      }

      service {
        name = "pingora-edge"
        port = "http"

        check {
          name     = "pingora-edge-alive"
          type     = "tcp"
          interval = "15s"
          timeout  = "5s"
        }
      }

      resources {
        cpu    = 500
        memory = 512
      }
    }
  }
}
