import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { constants } from "node:fs";
import {
  cp,
  chmod,
  mkdir,
  readFile,
  writeFile,
  realpath,
  lstat,
  access,
  statfs,
  open,
  unlink,
  rename,
} from "node:fs/promises";
import { homedir } from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { promisify, parseArgs } from "node:util";
import { readJson, privateJson } from "./common.mjs";
const exec = promisify(execFile);
export const sha = (b) => createHash("sha256").update(b).digest("hex");
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const within = (root, file) => path.resolve(file).startsWith(path.resolve(root) + path.sep);
export const runtimeName = "soapbox-0.3.0-rc.1-linux-x64.tar.gz";
export const runtimeHash = "03c4c2de8e42032c6710ce0b4f1170d893c2a36b26a0ad88398173cd16ff9588";
export async function verifyRelease(release) {
  const meta = await readJson(path.join(release, "release.json"));
  assert.equal(meta.chain_id, 84532, "Release must use Base Sepolia");
  assert.equal(meta.platform, "linux-x64");
  assert.equal(meta.installer_version, 2);
  assert.ok(meta.files && Object.keys(meta.files).length > 5);
  const root = await realpath(release);
  for (const [file, expected] of Object.entries(meta.files)) {
    assert.ok(!path.isAbsolute(file) && !file.split(/[\\/]/).some((p) => p === ".."));
    const target = path.join(root, file);
    assert.ok(
      within(root, await realpath(target)) && (await lstat(target)).isFile(),
      "Unsafe release path",
    );
    assert.equal(sha(await readFile(target)), expected, `Release integrity check failed: ${file}`);
  }
  for (const file of [
    "plugin/index.cjs",
    "plugin/bitterbot.plugin.json",
    "bin/soapbox",
    "ui/index.html",
    "setup/install.mjs",
    `runtime/${runtimeName}`,
  ])
    assert.ok(meta.files[file], `Missing release asset ${file}`);
  return meta;
}
export async function inspectHost() {
  const run = async (cmd, args) =>
    (
      await exec(cmd, args, {
        timeout: 15000,
        maxBuffer: 262144,
        env: { PATH: process.env.PATH, HOME: homedir(), LANG: "C" },
      })
    ).stdout.trim();
  const os = await readFile("/etc/os-release", "utf8").catch(() => "");
  const facts = {
    platform: process.platform,
    arch: process.arch,
    node: Number(process.versions.node.split(".")[0]),
    ubuntu: /^ID=ubuntu$/m.test(os) && /^VERSION_ID="24\.04"$/m.test(os),
  };
  try {
    const d = JSON.parse(await run("docker", ["info", "--format", "{{json .}}"]));
    facts.docker = Boolean(d.ServerVersion);
    facts.swap = d.SwapLimit === true;
    facts.cgroup = d.CgroupVersion === "2";
    facts.nvidiaRuntime = Boolean(d.Runtimes?.nvidia);
  } catch {
    facts.docker = false;
  }
  try {
    facts.gpus = await run(
      await access("/usr/lib/wsl/lib/nvidia-smi").then(
        () => "/usr/lib/wsl/lib/nvidia-smi",
        () => "nvidia-smi",
      ),
      ["--query-gpu=name", "--format=csv,noheader"],
    );
  } catch {
    facts.gpus = "";
  }
  return facts;
}
export async function preflight({ release, state, role = "buyer", facts }) {
  assert.ok(["buyer", "operator"].includes(role), "Choose Buyer or GPU operator");
  const checks = [],
    check = (name, ok, detail) => checks.push({ name, ok: Boolean(ok), detail });
  let meta, config;
  try {
    meta = await verifyRelease(release);
    check("Release integrity", true, `Soapbox ${meta.version} · Base Sepolia only`);
  } catch (e) {
    check("Release integrity", false, e.message);
  }
  facts ??= await inspectHost();
  check(
    "Supported system",
    facts.platform === "linux" && facts.arch === "x64" && facts.ubuntu,
    "Ubuntu 24.04 x64 or WSL2 Ubuntu 24.04 is required.",
  );
  check("Node runtime", facts.node >= 22, "Bitterbot must provide Node.js 22 or newer.");
  try {
    config = await readJson(path.join(state, "bitterbot.json"));
    assert.ok(
      config &&
        typeof config === "object" &&
        !Array.isArray(config) &&
        !JSON.stringify(config).includes('"$include"'),
      "Externally managed configuration requires manual integration",
    );
    const version = (await readJson(path.join(state, "app/current/package.json"))).version;
    check(
      "Bitterbot version",
      version === meta?.bitterbot_version,
      `Installed ${version}; requires ${meta?.bitterbot_version ?? "verified release"}.`,
    );
    await access(path.join(state, "bitterbot.json"), constants.R_OK | constants.W_OK);
    await access(state, constants.W_OK);
    check("Configuration access", true, "Existing settings and wallet data will be preserved.");
    check(
      "Gateway authentication",
      Boolean(config.gateway?.auth?.token || config.gateway?.auth?.password),
      "Configure authenticated Bitterbot access before installing.",
    );
    const space = await statfs(state),
      required = role === "operator" ? 4 * 1024 ** 3 : 256 * 1024 ** 2;
    check(
      "Free disk space",
      space.bavail * space.bsize >= required,
      role === "operator"
        ? "At least 4 GiB free for the runtime and model."
        : "At least 256 MiB free.",
    );
  } catch (e) {
    check(
      "Bitterbot configuration",
      false,
      e.message.startsWith("ENOENT") ? "Install and open Bitterbot 1.8.0 first." : e.message,
    );
  }
  if (role === "operator") {
    check(
      "Docker access",
      facts.docker,
      "Docker must be running and accessible to your WSL/Linux user.",
    );
    check(
      "Memory isolation",
      facts.swap && facts.cgroup,
      "Docker must enforce swap limits using cgroup v2.",
    );
    check(
      "NVIDIA GPU",
      facts.gpus,
      facts.gpus || "Install a compatible NVIDIA driver with WSL support if applicable.",
    );
    check(
      "NVIDIA container runtime",
      facts.nvidiaRuntime,
      "Install and configure NVIDIA Container Toolkit for Docker.",
    );
  }
  return {
    ok: checks.every((c) => c.ok),
    checks,
    role,
    version: meta?.version,
    chain_id: 84532,
    existing_compute: Boolean(config?.plugins?.entries?.["soapbox-compute"]),
  };
}
async function locked(state, action) {
  const dir = path.join(state, "compute");
  await mkdir(dir, { recursive: true, mode: 0o700 });
  const file = path.join(dir, "installer.lock");
  const lock = await open(file, "wx", 0o600).catch(() => {
    throw Error("Another installer is running. Close it before retrying.");
  });
  try {
    return await action();
  } finally {
    await lock.close();
    await unlink(file);
  }
}
async function atomicText(file, text) {
  const temp = file + ".next-" + randomUUID();
  await writeFile(temp, text, { mode: 0o600, flag: "wx" });
  await rename(temp, file);
}
export async function install({ release, state, role = "buyer", apply = false, facts }) {
  const plan = await preflight({ release, state, role, facts });
  assert.ok(plan.ok, "Prerequisite checks must pass before installation");
  if (!apply) return { ...plan, dry_run: true };
  return locked(state, async () => {
    const file = path.join(state, "bitterbot.json"),
      before = await readFile(file, "utf8"),
      config = JSON.parse(before),
      target = path.join(state, "compute", `soapbox-${plan.version}-${randomUUID()}`);
    await mkdir(target, { recursive: true, mode: 0o700 });
    for (const dir of ["plugin", "ui", "bin", "runtime"])
      await cp(path.join(release, dir), path.join(target, dir), { recursive: true });
    await chmod(path.join(target, "bin/soapbox"), 0o755);
    const meta = await verifyRelease(release);
    for (const [name, hash] of Object.entries(meta.files))
      if (/^(plugin|ui|bin|runtime)\//.test(name))
        assert.equal(sha(await readFile(path.join(target, name))), hash);
    config.plugins ??= {};
    config.plugins.load ??= {};
    config.plugins.entries ??= {};
    const removed = [],
      keep = [];
    for (const entry of config.plugins.load.paths ?? []) {
      const manifest = await readJson(path.join(entry, "bitterbot.plugin.json")).catch(() => null);
      (manifest?.id === "soapbox-compute" ? removed : keep).push(entry);
    }
    config.plugins.load.paths = [...keep, path.join(target, "plugin")];
    const existing = config.plugins.entries["soapbox-compute"];
    const active =
      existing?.config &&
      (["participant", "pilot"].includes(existing.config.mode) || existing.config.repository);
    config.plugins.entries["soapbox-compute"] = active
      ? existing
      : {
          enabled: true,
          config: {
            mode: "setup",
            bundledRuntimeArchive: path.join(target, "runtime", runtimeName),
          },
        };
    if (Array.isArray(config.plugins.allow) && !config.plugins.allow.includes("soapbox-compute"))
      config.plugins.allow.push("soapbox-compute");
    config.gateway ??= {};
    config.gateway.controlUi = {
      ...config.gateway.controlUi,
      enabled: true,
      root: path.join(target, "ui"),
    };
    assert.deepEqual(config.tools?.wallet, JSON.parse(before).tools?.wallet);
    const after = JSON.stringify(config, null, 2) + "\n";
    await writeFile(path.join(target, "before-config.json"), before, { mode: 0o600 });
    await writeFile(path.join(target, "after-config.json"), after, { mode: 0o600 });
    await privateJson(path.join(target, "installed.json"), {
      version: plan.version,
      state: path.resolve(state),
      target,
      removed,
      config_sha256: sha(after),
      chain_id: 84532,
    });
    assert.equal(
      await readFile(file, "utf8"),
      before,
      "Settings changed during installation; retry",
    );
    await atomicText(file, after);
    await privateJson(path.join(state, "compute/installer-latest.json"), { target });
    return { ...plan, installed: true, target, main_wallet_changed: false };
  });
}
export async function latestInstall(state) {
  // Recover a completed config swap even if the process stopped before writing the latest marker.
  try {
    const config = await readJson(path.join(state, "bitterbot.json")),
      target = path.dirname(config.gateway.controlUi.root);
    assert.ok(within(path.join(state, "compute"), target));
    const info = await readJson(path.join(target, "installed.json"));
    assert.equal(info.state, path.resolve(state));
    assert.equal(info.target, target);
    return target;
  } catch {}
  try {
    const { target } = await readJson(path.join(state, "compute/installer-latest.json"));
    assert.ok(within(path.join(state, "compute"), target));
    return target;
  } catch {
    return null;
  }
}
export async function rollback(state, target) {
  assert.ok(
    within(path.join(state, "compute"), target),
    "Rollback must target this Bitterbot installation",
  );
  return locked(state, async () => {
    const info = await readJson(path.join(target, "installed.json"));
    assert.equal(info.state, path.resolve(state));
    assert.equal(info.target, target);
    const file = path.join(state, "bitterbot.json"),
      currentText = await readFile(file, "utf8"),
      current = JSON.parse(currentText);
    const beforeText = await readFile(path.join(target, "before-config.json"), "utf8"),
      before = JSON.parse(beforeText),
      installed = await readJson(path.join(target, "after-config.json"));
    if (same(current, before)) return { rolled_back: true };
    assert.equal(
      current.gateway?.controlUi?.root,
      path.join(target, "ui"),
      "Compute UI was changed elsewhere; automatic rollback is unavailable",
    );
    const entry = current.plugins?.entries?.["soapbox-compute"];
    if (!same(entry, installed.plugins.entries["soapbox-compute"])) {
      assert.equal(
        installed.plugins.entries["soapbox-compute"].config.mode,
        "setup",
        "Existing Compute configuration changed; resolve it before rollback",
      );
      assert.equal(
        entry?.config?.mode,
        "participant",
        "Compute configuration changed outside the wizard",
      );
      assert.ok(within(path.join(state, "compute/setup"), entry.config.stateDirectory));
      const backup = await readJson(
        path.join(entry.config.stateDirectory, "bitterbot.before.json"),
      );
      assert.deepEqual(
        backup.plugins.entries["soapbox-compute"],
        installed.plugins.entries["soapbox-compute"],
      );
      const connection = await readJson(path.join(entry.config.stateDirectory, "connection.json"));
      assert.equal(connection.chain_id, 84532);
      const response = await fetch(new URL("/v1/status", connection.url), {
        headers: { Authorization: "Bearer " + connection.token },
        redirect: "error",
        signal: AbortSignal.timeout(10000),
      });
      assert.ok(response.ok, "Coordinator must be reachable to check unfinished jobs");
      const status = await response.json();
      assert.ok(
        !status.jobs.some((j) =>
          [
            "awaiting_payment",
            "queued",
            "running",
            "settlement_pending",
            "refund_pending",
            "payment_review",
          ].includes(j.status),
        ),
        "Finish or cancel pending jobs and wait for settlement before rollback",
      );
    }
    const restored = structuredClone(current),
      prior = before.plugins?.entries?.["soapbox-compute"];
    if (prior === undefined) delete restored.plugins.entries["soapbox-compute"];
    else restored.plugins.entries["soapbox-compute"] = prior;
    restored.plugins.load.paths = restored.plugins.load.paths.filter(
      (p) => p !== path.join(target, "plugin"),
    );
    for (const p of info.removed)
      if (!restored.plugins.load.paths.includes(p)) restored.plugins.load.paths.push(p);
    if (
      !before.plugins?.allow?.includes("soapbox-compute") &&
      Array.isArray(restored.plugins.allow)
    )
      restored.plugins.allow = restored.plugins.allow.filter((p) => p !== "soapbox-compute");
    for (const key of ["root", "enabled"]) {
      if (before.gateway?.controlUi?.[key] === undefined) delete restored.gateway.controlUi[key];
      else restored.gateway.controlUi[key] = before.gateway.controlUi[key];
    }
    for (const [parent, key, priorValue] of [
      [restored.plugins.load, "paths", before.plugins?.load?.paths],
      [restored.plugins, "load", before.plugins?.load],
      [restored.plugins, "entries", before.plugins?.entries],
      [restored, "plugins", before.plugins],
      [restored.gateway, "controlUi", before.gateway?.controlUi],
      [restored, "gateway", before.gateway],
    ])
      if (priorValue === undefined && parent[key] && Object.keys(parent[key]).length === 0)
        delete parent[key];
    assert.deepEqual(restored.tools, current.tools);
    assert.equal(
      await readFile(file, "utf8"),
      currentText,
      "Settings changed during rollback; retry",
    );
    await atomicText(
      file,
      same(restored, before) ? beforeText : JSON.stringify(restored, null, 2) + "\n",
    );
    return { rolled_back: true, main_wallet_changed: false };
  });
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const { values: v } = parseArgs({
    options: {
      role: { type: "string", default: "buyer" },
      state: { type: "string", default: path.join(homedir(), ".bitterbot") },
      apply: { type: "boolean" },
      rollback: { type: "string" },
    },
  });
  console.log(
    JSON.stringify(
      v.rollback
        ? await rollback(v.state, v.rollback)
        : await install({
            release: path.resolve(import.meta.dirname, ".."),
            state: v.state,
            role: v.role,
            apply: v.apply,
          }),
      null,
      2,
    ),
  );
}
