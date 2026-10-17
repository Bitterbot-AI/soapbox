/** Prepare only operator-owned runtime assets. Never downloads buyer-supplied models or images. */
import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { existsSync } from "node:fs";
import { mkdir, copyFile } from "node:fs/promises";
import { homedir } from "node:os";
import path from "node:path";
import { parseArgs } from "node:util";
import { privateJson } from "./common.mjs";
import { verifyRelease } from "./install.mjs";
const { values: v } = parseArgs({
  options: {
    directory: { type: "string", default: path.join(homedir(), ".bitterbot-soapbox/operator") },
    "model-cache": { type: "string" },
  },
});
assert.equal(process.platform, "linux");
assert.equal(process.arch, "x64");
const root = path.resolve(import.meta.dirname, "..");
await verifyRelease(root);
const directory = path.resolve(v.directory),
  models = path.resolve(v["model-cache"] ?? path.join(directory, "models"));
assert.ok(!models.includes(",") && !models.includes("\n"));
const docker = (args) =>
  execFileSync("docker", args, { encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] }).trim();
assert.equal(
  docker(["info", "--format", "{{.SwapLimit}}"]),
  "true",
  "Docker must enforce swap limits",
);
execFileSync(
  existsSync("/usr/lib/wsl/lib/nvidia-smi") ? "/usr/lib/wsl/lib/nvidia-smi" : "nvidia-smi",
  ["--query-gpu=name", "--format=csv,noheader"],
  { stdio: "inherit" },
);
await mkdir(directory, { recursive: true, mode: 0o700 });
await mkdir(models, { recursive: true });
const build = path.join(directory, "image");
await mkdir(build, { recursive: true });
await copyFile(path.join(root, "bin/soapbox"), path.join(build, "soapbox"));
await copyFile(path.join(root, "containers/Dockerfile"), path.join(build, "Dockerfile"));
execFileSync(
  "docker",
  [
    "buildx",
    "build",
    "--builder",
    "default",
    "--load",
    "--tag",
    "soapbox-isolated:0.3.0-rc.1",
    build,
  ],
  { stdio: "inherit" },
);
const image = docker(["image", "inspect", "soapbox-isolated:0.3.0-rc.1", "--format", "{{.Id}}"]);
if (!v["model-cache"]) {
  const name = `soapbox-setup-${randomUUID()}`;
  try {
    docker([
      "run",
      "-d",
      "--name",
      name,
      "--user",
      `${process.getuid()}:${process.getgid()}`,
      "--cap-drop=ALL",
      "--security-opt=no-new-privileges:true",
      "--memory=2g",
      "--memory-swap=2g",
      "--pids-limit=128",
      "--env",
      "OLLAMA_MODELS=/models",
      "--env",
      "OLLAMA_NO_CLOUD=1",
      "--mount",
      `type=bind,source=${models},target=/models`,
      "--entrypoint=/bin/ollama",
      image,
      "serve",
    ]);
    let ready = false;
    for (let i = 0; i < 30; i++) {
      try {
        docker(["exec", name, "/bin/ollama", "list"]);
        ready = true;
        break;
      } catch {
        await new Promise((r) => setTimeout(r, 1000));
      }
    }
    assert.ok(ready, "Model download runtime did not start");
    execFileSync("docker", ["exec", name, "/bin/ollama", "pull", "all-minilm:latest"], {
      stdio: "inherit",
    });
  } finally {
    docker(["rm", "-f", name]);
  }
}
const config = {
  modes: ["paid", "private", "collective"],
  pools: [],
  projects: [],
  memory_mib: 1024,
  ollama_model: "all-minilm:latest",
  ollama_digest: "1b226e2802dbb772b5fc32a58f103ca1804ef7501331012de126ab22f67475ef",
  max_gpu_temperature_c: 80,
  max_gpu_utilization_pct: 40,
  min_gpu_free_mib: 256,
  pause_file: path.join(directory, "PAUSE"),
  min_price_credits: 1,
  allow_unsafe_host: false,
  sandbox: { image, models, gpu_lock: path.join(homedir(), ".bitterbot-soapbox/gpu.lock") },
};
// Pool memberships are opt-in; the coordinator still limits them to the enrollment ACL.
const filename = path.join(directory, "worker.json");
await privateJson(filename, config);
const probe = JSON.parse(
  execFileSync(path.join(root, "bin/soapbox"), ["inspect", "--config", filename], {
    encoding: "utf8",
    timeout: 180000,
    stdio: ["ignore", "pipe", "inherit"],
  }),
);
assert.ok(
  probe.gpu_observed && probe.isolation === "docker_v1",
  `GPU readiness failed: ${probe.gpu_unavailable_reason ?? "inspect Docker/NVIDIA runtime"}`,
);
console.log(
  JSON.stringify(
    {
      worker_config: filename,
      gpu: probe.hardware.map((g) => g.name),
      model: probe.runtime?.model,
      chain_id: 84532,
    },
    null,
    2,
  ),
);
