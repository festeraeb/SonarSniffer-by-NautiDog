job "vector-agent" {
  datacenters = ["dc1"]
  type = "system"

  group "agent" {
    network {
      port "api" {
        static = 8686
      }
    }

    constraint {
      attribute = "${attr.driver.raw_exec}"
      value     = "1"
    }

    task "vector" {
      driver = "raw_exec"

      config {
        command = "/usr/local/bin/vector"
        args = ["--config", "${NOMAD_TASK_DIR}/vector.toml"]
      }

      template {
        destination = "${NOMAD_TASK_DIR}/vector.toml"
        data = <<EOF
data_dir = "${NOMAD_ALLOC_DIR}"

[sources.journald]
type = "journald"

[transforms.add_labels]
type = "remap"
inputs = ["journald"]
source = '''
  .node = get_env_var("NOMAD_NODE_NAME") ?? "unknown"
  .job = get_env_var("NOMAD_JOB_NAME") ?? "unknown"
'''

[sinks.console]
type = "console"
inputs = ["add_labels"]
encoding.codec = "json"
EOF
      }

      resources {
        cpu    = 200
        memory = 256
      }
    }
  }
}
