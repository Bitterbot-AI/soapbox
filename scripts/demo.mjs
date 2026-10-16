#!/usr/bin/env node
import assert from "node:assert/strict";
import { randomBytes, randomUUID, createHash } from "node:crypto";
import { spawn } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = fileURLToPath(new URL("../", import.meta.url));
const binary = process.env.SOAPBOX_BINARY || path.join(root, "target", "debug", process.platform === "win32" ? "soapbox.exe" : "soapbox");
const url = process.env.SOAPBOX_DEMO_URL || "http://127.0.0.1:8787";
const address = new URL(url);
assert.equal(address.protocol, "http:");
assert.equal(address.hostname, "127.0.0.1");
const keep = process.argv.includes("--keep");
const token = () => randomBytes(32).toString("hex");
const env = { ...process.env, SOAPBOX_ADMIN_TOKEN: token(), SOAPBOX_CLIENT_TOKEN: token(),
  SOAPBOX_WORKER_TOKEN: token(), SOAPBOX_WORKER2_TOKEN: token() };
const children = [];
let startupError;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
function start(args, extra = {}) {
  const child = spawn(binary, args, { cwd: root, env: { ...env, ...extra }, shell: false,
    windowsHide: true, stdio: ["ignore", "ignore", "pipe"] });
  child.stderr.on("data", (data) => process.stderr.write(data));
  child.on("error", (error) => { startupError = error; });
  children.push(child);
  return child;
}
async function stop() {
  for (const child of children.toReversed()) {
    if (child.exitCode !== null || child.signalCode !== null) continue;
    child.kill("SIGTERM");
    await Promise.race([new Promise((resolve) => child.once("exit", resolve)), sleep(1000)]);
    if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
  }
}
async function request(route, body, credential = env.SOAPBOX_CLIENT_TOKEN) {
  const response = await fetch(new URL(route, url), {
    method: body === undefined ? "GET" : "POST",
    headers: { Authorization: "Bearer " + credential, "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(3000), redirect: "error",
  });
  const data = await response.json();
  assert.equal(response.ok, true, JSON.stringify(data));
  return data;
}
try {
  await mkdir(path.join(root, ".local"), { recursive: true });
  const db = path.join(root, ".local", "demo-" + randomUUID() + ".sqlite");
  const server = start(["serve", "--config", "examples/coordinator.json", "--db", db, "--bind", "127.0.0.1:" + (address.port || "80")]);
  let ready = false;
  for (let i = 0; i < 40; i++) {
    if (startupError) throw startupError;
    if (server.exitCode !== null) throw new Error("Demo coordinator exited; check whether its port is already in use");
    try { await request("/v1/status"); ready = true; break; } catch { await sleep(150); }
  }
  assert.ok(ready, "Coordinator did not become ready");
  await request("/v1/credits", { account: "buyer", amount: 100, idempotency_key: "demo-funding" }, env.SOAPBOX_ADMIN_TOKEN);
  start(["worker", "--config", "examples/worker.json", "--coordinator", url]);
  start(["worker", "--config", "examples/worker.json", "--coordinator", url, "--token-env", "SOAPBOX_WORKER2_TOKEN"]);
  const modes = [{ kind: "paid", max_cost_credits: 2 }, { kind: "private", pool_id: "circle-demo" }, { kind: "collective", project_id: "research-demo" }];
  const jobs = [];
  for (const mode of modes) {
    const body = { idempotency_key: "demo-" + mode.kind, mode, workload: { kind: "sha256", texts: ["Soapbox", "Bitterbot"] } };
    const job = await request("/v1/jobs", body);
    assert.equal((await request("/v1/jobs", body)).id, job.id);
    jobs.push(job);
  }
  const finished = [];
  for (const job of jobs) {
    let latest;
    for (let i = 0; i < 80; i++) {
      latest = await request("/v1/jobs/" + job.id);
      if (["succeeded", "failed", "cancelled"].includes(latest.status)) break;
      await sleep(250);
    }
    assert.equal(latest.status, "succeeded");
    assert.equal(latest.result.hashes[0], createHash("sha256").update("Soapbox").digest("hex"));
    finished.push({ mode: latest.spec.mode.kind, status: latest.status, receipt: latest.receipt });
  }
  const artifact = await request("/v1/artifacts", { content: "Soapbox temporary artifact", ttl_seconds: 3600 });
  assert.equal((await request("/v1/artifacts/" + artifact.id)).content, "Soapbox temporary artifact");
  const status = await request("/v1/status");
  assert.equal(status.account.available_credits, 98);
  assert.equal(status.account.reserved_credits, 0);
  console.log(JSON.stringify({ passed: true, jobs: finished, account: status.account, artifact, database: db }, null, 2));
  if (keep) {
    const settings = "BITTERBOT_SOAPBOX_ENABLED=1\nBITTERBOT_SOAPBOX_URL=" + url + "\nBITTERBOT_SOAPBOX_TOKEN=" + env.SOAPBOX_CLIENT_TOKEN + "\n";
    const envFile = path.join(root, ".local", "demo.env");
    await writeFile(envFile, settings, { mode: 0o600 });
    console.log("Demo remains running. Gateway connection settings saved to " + envFile + ". Press Ctrl+C to stop.");
    await new Promise((resolve) => { process.once("SIGINT", resolve); process.once("SIGTERM", resolve); });
  }
} finally { await stop(); }
