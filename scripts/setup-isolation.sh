#!/usr/bin/env bash
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
[[ $(docker info --format '{{.SwapLimit}}') == true ]] || { echo "Docker must enforce swap limits. See docs/PHASE-2-ISOLATION-PAYMENTS.md." >&2; exit 1; }
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-/tmp/soapbox-target}
cargo build --locked
build_dir=$(mktemp -d /tmp/soapbox-image.XXXXXX)
trap 'rm -rf -- "$build_dir"' EXIT
cp "$CARGO_TARGET_DIR/debug/soapbox" "$build_dir/soapbox"
cp containers/Dockerfile "$build_dir/Dockerfile"
docker buildx build --builder default --load -t soapbox-isolated:dev "$build_dir"
python3 - "$root" "${SOAPBOX_MODEL_CACHE:-$root/.local/models}" <<'PY'
from pathlib import Path
import json,subprocess,sys
root=Path(sys.argv[1]);models=Path(sys.argv[2]).resolve()
if not models.is_dir(): raise SystemExit('Install the pinned all-minilm model before setup; model cache missing.')
output=root/'.local/worker-isolated.json'
source=output if output.exists() else root/'examples/worker-gpu.json'
cfg=json.loads(source.read_text(encoding='utf-8-sig'))
cfg.update(memory_mib=2048,allow_unsafe_host=False,pause_file=str(root/'.local/PAUSE'))
cfg['sandbox']={'image':subprocess.check_output(['docker','image','inspect','soapbox-isolated:dev','--format','{{.Id}}'],text=True).strip(),'models':str(models),'gpu_lock':str(root/'.local/gpu.lock')}
output.parent.mkdir(exist_ok=True);output.write_text(json.dumps(cfg,indent=2)+'\n')
print('Worker config:',output)
PY
"$CARGO_TARGET_DIR/debug/soapbox" inspect --config .local/worker-isolated.json > .local/isolation-inspect.json
cat .local/isolation-inspect.json
python3 -c 'import json; s=json.load(open(".local/isolation-inspect.json")); assert s["gpu_observed"] and s["isolation"] == "docker_v1", "GPU isolation probe failed; inspect the diagnostic above"'
