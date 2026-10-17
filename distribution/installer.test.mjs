import assert from "node:assert/strict";
import { mkdtemp, mkdir, writeFile, readFile, rm, symlink } from "node:fs/promises";
import { createServer, request } from "node:http";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";
import { privateJson, readJson } from "./common.mjs";
import {
  install,
  rollback,
  preflight,
  sha,
  runtimeName,
  verifyRelease,
  latestInstall,
} from "./install.mjs";
import { createInstallerServer } from "./installer-server.mjs";
const facts = {
  platform: "linux",
  arch: "x64",
  node: 22,
  ubuntu: true,
  docker: true,
  swap: true,
  cgroup: true,
  nvidiaRuntime: true,
  gpus: "Test GPU",
};
async function fixture(t) {
  const root = await mkdtemp(path.join(tmpdir(), "soapbox-installer-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const release = path.join(root, "release"),
    state = path.join(root, "state"),
    files = {};
  for (const file of [
    "plugin/index.cjs",
    "plugin/bitterbot.plugin.json",
    "bin/soapbox",
    "ui/index.html",
    "setup/install.mjs",
    `runtime/${runtimeName}`,
  ]) {
    const data = file.endsWith(".json") ? '{"id":"soapbox-compute"}' : "fixture";
    await mkdir(path.dirname(path.join(release, file)), { recursive: true });
    await writeFile(path.join(release, file), data);
    files[file] = sha(data);
  }
  await privateJson(path.join(release, "release.json"), {
    version: "0.3.0-rc.2",
    installer_version: 2,
    platform: "linux-x64",
    chain_id: 84532,
    bitterbot_version: "1.8.0",
    files,
  });
  await privateJson(path.join(state, "app/current/package.json"), { version: "1.8.0" });
  const original = {
    tools: { wallet: { network: "base", walletStorePath: "/mainnet", dailySpendLimitUsd: 5 } },
    agents: { preserve: true },
    gateway: { auth: { token: "existing-gateway" }, port: 19001 },
  };
  await privateJson(path.join(state, "bitterbot.json"), original);
  await privateJson(path.join(state, "wallet/wallet-data.json"), { identity: "must-not-change" });
  return { release, state, root, original, facts };
}
test("buyer needs no GPU; operator requires Docker, NVIDIA and memory isolation", async (t) => {
  const f = await fixture(t),
    before = await readFile(path.join(f.state, "bitterbot.json"), "utf8");
  const missing = { ...facts, docker: false, swap: false, nvidiaRuntime: false, gpus: "" };
  assert.equal((await preflight({ ...f, role: "buyer", facts: missing })).ok, true);
  const operator = await preflight({ ...f, role: "operator", facts: missing });
  assert.equal(operator.ok, false);
  assert.equal(operator.checks.filter((c) => !c.ok).length, 4);
  for (const change of [{ platform: "win32" }, { node: 20 }, { ubuntu: false }])
    assert.equal((await preflight({ ...f, facts: { ...facts, ...change } })).ok, false);
  assert.equal(await readFile(path.join(f.state, "bitterbot.json"), "utf8"), before);
});
test("clean install is enrollment-free and rollback restores exact settings without touching wallet data", async (t) => {
  const f = await fixture(t),
    file = path.join(f.state, "bitterbot.json"),
    before = await readFile(file, "utf8"),
    wallet = await readFile(path.join(f.state, "wallet/wallet-data.json"), "utf8");
  const installed = await install({ ...f, apply: true });
  const config = await readJson(file);
  assert.equal(config.plugins.entries["soapbox-compute"].config.mode, "setup");
  assert.ok(config.plugins.entries["soapbox-compute"].config.bundledRuntimeArchive);
  assert.deepEqual(config.tools, f.original.tools);
  await rm(path.join(f.state, "compute/installer-latest.json"));
  assert.equal(await latestInstall(f.state), installed.target);
  await rollback(f.state, installed.target);
  assert.equal(await readFile(file, "utf8"), before);
  assert.equal(await readFile(path.join(f.state, "wallet/wallet-data.json"), "utf8"), wallet);
});
test("rollback preserves later wallet and unrelated plugin changes", async (t) => {
  const f = await fixture(t),
    installed = await install({ ...f, apply: true }),
    file = path.join(f.state, "bitterbot.json"),
    config = await readJson(file);
  config.tools.wallet.dailySpendLimitUsd = 7;
  config.channels = { test: { enabled: true } };
  config.plugins.entries.other = { enabled: true };
  config.plugins.load.paths.push("/unrelated");
  await privateJson(file, config);
  await rollback(f.state, installed.target);
  const restored = await readJson(file);
  assert.equal(restored.tools.wallet.dailySpendLimitUsd, 7);
  assert.deepEqual(restored.channels, config.channels);
  assert.equal(restored.plugins.entries.other.enabled, true);
  assert.deepEqual(restored.plugins.load.paths, ["/unrelated"]);
  assert.equal(restored.plugins.entries["soapbox-compute"], undefined);
});
test("rollback refuses conflicting UI ownership and another profile", async (t) => {
  const f = await fixture(t),
    installed = await install({ ...f, apply: true }),
    file = path.join(f.state, "bitterbot.json"),
    config = await readJson(file);
  config.gateway.controlUi.root = "/someone-else";
  await privateJson(file, config);
  await assert.rejects(rollback(f.state, installed.target), /changed elsewhere/);
  await assert.rejects(rollback(f.root, installed.target), /this Bitterbot/);
});
test("rollback after wizard activation checks unfinished jobs and retains credentials on disk", async (t) => {
  const f = await fixture(t),
    installed = await install({ ...f, apply: true }),
    file = path.join(f.state, "bitterbot.json"),
    config = await readJson(file),
    participant = path.join(f.state, "compute/setup/participant-test");
  let pending = true;
  const server = createServer((req, res) => {
    res.setHeader("Content-Type", "application/json");
    res.end(JSON.stringify({ jobs: pending ? [{ status: "running" }] : [] }));
  });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  t.after(() => server.close());
  await privateJson(path.join(participant, "bitterbot.before.json"), config);
  await privateJson(path.join(participant, "connection.json"), {
    chain_id: 84532,
    url: `http://127.0.0.1:${server.address().port}`,
    token: "private-test-token",
  });
  config.plugins.entries["soapbox-compute"] = {
    enabled: true,
    config: { mode: "participant", stateDirectory: participant },
  };
  await privateJson(file, config);
  await assert.rejects(rollback(f.state, installed.target), /pending jobs/);
  pending = false;
  await rollback(f.state, installed.target);
  assert.deepEqual(await readJson(file), f.original);
  assert.ok(await readFile(path.join(participant, "connection.json")));
});
test("tampered release, mainnet manifest, wrong desktop and managed config fail before mutation", async (t) => {
  const f = await fixture(t),
    file = path.join(f.state, "bitterbot.json"),
    before = await readFile(file, "utf8");
  await privateJson(path.join(f.state, "app/current/package.json"), { version: "1.9.0" });
  assert.equal((await preflight(f)).ok, false);
  await privateJson(path.join(f.state, "app/current/package.json"), { version: "1.8.0" });
  await privateJson(file, { $include: "external.json" });
  assert.equal((await preflight(f)).ok, false);
  await writeFile(file, before);
  await writeFile(path.join(f.release, "bin/soapbox"), "tampered");
  await assert.rejects(install({ ...f, apply: true }), /Prerequisite/);
  assert.equal(await readFile(file, "utf8"), before);
  const meta = await readJson(path.join(f.release, "release.json"));
  meta.chain_id = 8453;
  await privateJson(path.join(f.release, "release.json"), meta);
  await assert.rejects(verifyRelease(f.release), /Base Sepolia/);
});
test("local installer requires session authentication and rejects foreign Origin/Host", async (t) => {
  const f = await fixture(t),
    app = await createInstallerServer(f);
  t.after(() => {
    app.server.close();
    app.server.closeAllConnections();
  });
  assert.equal((await fetch(app.origin + "/api/status")).status, 403);
  const headers = { Authorization: "Bearer " + app.secret, "Content-Type": "application/json" };
  assert.equal(
    (
      await fetch(app.origin + "/api/install", {
        method: "POST",
        headers: { ...headers, Origin: "https://attacker.example" },
        body: "{}",
      })
    ).status,
    403,
  );
  const badHost = await new Promise((resolve, reject) => {
    const req = request(
      app.origin + "/api/status",
      { headers: { ...headers, Host: "attacker.example" } },
      (res) => {
        res.resume();
        resolve(res.statusCode);
      },
    );
    req.on("error", reject);
    req.end();
  });
  assert.equal(badHost, 403);
  assert.equal(
    (
      await fetch(app.origin + "/api/install", {
        method: "POST",
        headers,
        body: JSON.stringify({ role: "admin" }),
      })
    ).status,
    400,
  );
  const response = await fetch(app.origin + "/api/install", {
    method: "POST",
    headers,
    body: JSON.stringify({ role: "buyer" }),
  });
  assert.equal(response.status, 200);
  assert.equal((await response.json()).installed, true);
  const restored = await fetch(app.origin + "/api/rollback", {
    method: "POST",
    headers,
    body: "{}",
  });
  assert.equal(restored.status, 200);
  assert.deepEqual(await readJson(path.join(f.state, "bitterbot.json")), f.original);
});
