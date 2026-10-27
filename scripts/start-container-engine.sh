#!/usr/bin/env bash
set -euo pipefail
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
# Foreground helper for WSL installations without systemd. Does not stop another daemon.
if timeout 10 docker info >/dev/null 2>&1; then
  [[ $(docker info --format '{{.CgroupVersion}} {{.SwapLimit}}') == '2 true' ]] || {
    echo 'Docker is running without required isolation. Arrange a controlled service restart; do not launch a second daemon.' >&2
    exit 1
  }
  echo 'Docker is already running with cgroup v2 and swap enforcement.'
  exit 0
fi
[[ $(id -u) == 0 ]] || { echo "Run this helper as root inside WSL." >&2; exit 1; }
! pgrep -x dockerd >/dev/null || { echo 'Docker exists but is not ready; refusing a competing daemon.' >&2; exit 1; }
[[ -f /sys/fs/cgroup/cgroup.controllers ]] || { echo 'cgroup v2 is required.' >&2; exit 1; }
echo '+memory +pids +cpu' > /sys/fs/cgroup/cgroup.subtree_control
mkdir -p /sys/fs/cgroup/soapbox-engine
# Root cgroups do not expose memory.swap.max; Docker must probe a non-root cgroup.
echo $$ > /sys/fs/cgroup/soapbox-engine/cgroup.procs
exec dockerd --host=unix:///var/run/docker.sock --exec-opt native.cgroupdriver=cgroupfs
