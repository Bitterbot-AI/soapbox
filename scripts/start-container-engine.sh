#!/usr/bin/env bash
set -euo pipefail
# Foreground helper for WSL installations without systemd. Does not stop another daemon.
if docker info >/dev/null 2>&1; then
  echo "Docker is already running; swap enforcement: $(docker info --format '{{.SwapLimit}}')"
  exit 0
fi
[[ $(id -u) == 0 ]] || { echo "Run this helper as root inside WSL." >&2; exit 1; }
mkdir -p /sys/fs/cgroup/soapbox-engine
# Root cgroups do not expose memory.swap.max; Docker must probe a non-root cgroup.
echo $$ > /sys/fs/cgroup/soapbox-engine/cgroup.procs
exec dockerd --host=unix:///var/run/docker.sock --exec-opt native.cgroupdriver=cgroupfs
