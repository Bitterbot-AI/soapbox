import assert from "node:assert/strict";
import { randomBytes } from "node:crypto";
import { access, open, unlink } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { parseArgs } from "node:util";
import { readJson, privateJson, connection, PUBLIC_SEPOLIA_RPC } from "./common.mjs";

export async function enroll({ state, role, id, address, url, output, pools = [], projects = [] }) {
  assert.ok(!["buyer", "worker"].includes(id), "Choose a unique participant ID");
  assert.ok(Array.isArray(pools) && Array.isArray(projects));
  for (const v of [...pools, ...projects]) assert.match(v, /^[a-zA-Z0-9_-]{1,128}$/);
  assert.notEqual(path.resolve(output), path.join(path.resolve(state), "admissions.json"));
  await access(output).then(
    () => {
      throw new Error("Enrollment output already exists");
    },
    (e) => {
      if (e.code !== "ENOENT") throw e;
    },
  );
  const lock = path.join(state, "enrollment.lock");
  const handle = await open(lock, "wx", 0o600);
  try {
    const config = await readJson(path.join(state, "coordinator.json"));
    const admissions = await readJson(path.join(state, "admissions.json")).catch((e) => {
      if (e.code === "ENOENT") return [];
      throw e;
    });
    assert.ok(
      !admissions.some((a) => a.id === id) && !config.principals.some((p) => p.id === id),
      "Identity already enrolled; revoke it before rotating",
    );
    const c = connection({
      version: 1,
      role,
      id,
      address,
      url,
      token: randomBytes(32).toString("hex"),
      chain_id: config.payments.chain_id,
      escrow: config.payments.escrow,
      escrow_code_sha256: config.payments.escrow_code_sha256,
      // Coordinator RPC configuration is private and must never be exported.
      rpc_url: PUBLIC_SEPOLIA_RPC,
    });
    // Export first: failure to persist admissions leaves an inert credential, never an undisclosed active one.
    await privateJson(output, c);
    await privateJson(path.join(state, "admissions.json"), [
      ...admissions,
      { id, role, address, token: c.token, pools, projects },
    ]);
    return { id, role, address, chain_id: 84532, output };
  } finally {
    await handle.close();
    await unlink(lock);
  }
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const { values } = parseArgs({
    options: Object.fromEntries(
      ["state", "role", "id", "address", "url", "output", "pool", "project"].map((k) => [
        k,
        { type: "string" },
      ]),
    ),
  });
  const result = await enroll({
    ...values,
    pools: values.pool ? [values.pool] : [],
    projects: values.project ? [values.project] : [],
  });
  console.log(JSON.stringify(result, null, 2));
  console.log(
    "Enrollment saved. Restart the coordinator before testing. Transfer only the output file privately to this participant.",
  );
}
