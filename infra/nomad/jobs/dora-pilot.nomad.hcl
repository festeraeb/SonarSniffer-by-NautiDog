job "dora-pilot" {
  datacenters = ["dc1"]
  type = "service"

  group "graph" {
    count = 1

    task "dora-runtime" {
      driver = "raw_exec"

      config {
        command = "/codebase/repos/wreckhunter2000-1/target/release/cesarops-dora-runner"
        args = [
          "--graph",
          "/codebase/repos/wreckhunter2000-1/infra/nomad/dora/mission_pilot_graph.yaml"
        ]
      }

      resources {
        cpu    = 500
        memory = 512
      }
    }
  }
}
