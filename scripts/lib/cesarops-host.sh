# shellcheck shell=bash
# Source from fleet scripts: . "$(dirname "$0")/lib/cesarops-host.sh" with adjusted path

cesarops_node_name() {
  local hn
  hn="$(hostname -s | tr '[:upper:]' '[:lower:]')"
  hn="${hn%%.*}"
  if [[ "$hn" == *t440* ]]; then
    echo "t440"
  else
    echo "cesarops2"
  fi
}

# T440 must not run or restart local Forge (:9100); primary is cesarops2.
t440_forge_removed() {
  [[ "$(cesarops_node_name)" == "t440" ]] && return 0
  return 1
}

primary_forge_url() {
  echo "http://10.0.0.201:9100"
}
