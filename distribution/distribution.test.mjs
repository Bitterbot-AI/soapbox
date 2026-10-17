import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, mkdir, writeFile, readFile, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";
import { connection, privateJson, readJson } from "./common.mjs";
import { enroll } from "./enroll.mjs";
import { install, rollback } from "./install.mjs";
const c = {
  version: 1,
  role: "buyer",
  id: "test-buyer",
  chain_id: 84532,
  address: `0x${"1".repeat(40)}`,
  escrow: `0x${"2".repeat(40)}`,
  escrow_code_sha256: "a".repeat(64),
  url: "https://compute.example",
  rpc_url: "https://sepolia.base.org",
  token: "b".repeat(64),
};
test("enrollment rejects mainnet, cleartext remote credentials and embedded credentials", () => {
  for (const patch of [
    { chain_id: 8453 },
    { url: "http://remote.example" },
    { url: "https://user:pass@example.com" },
    { role: "admin" },
    { address: `0x${"0".repeat(40)}` },
  ])
    assert.throws(() => connection({ ...c, ...patch }));
  assert.equal(connection({ ...c, unwantedSecret: "drop-me" }).unwantedSecret, undefined);
});
test("scoped enrollment exports no coordinator or wallet secrets and rejects duplicate IDs", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "soapbox-enroll-"));
  await privateJson(path.join(root, "coordinator.json"), {
    payments: { ...c, chain_id: 84532, wallets: { buyer: c.address } },
    principals: [{ id: "buyer", token: "coordinator-secret" }],
  });
  const out = path.join(root, "operator.json");
  await enroll({
    state: root,
    role: "operator",
    id: "test-worker",
    address: c.address,
    url: c.url,
    output: out,
  });
  const value = await readJson(out);
  assert.equal(value.role, "operator");
  assert.equal(value.chain_id, 84532);
  assert.equal(value.wallets, undefined);
  assert.ok(!(await readFile(out, "utf8")).includes("coordinator-secret"));
  assert.equal((await stat(out)).mode & 0o777, 0o600);
  await assert.rejects(
    enroll({
      state: root,
      role: "operator",
      id: "test-worker",
      address: c.address,
      url: c.url,
      output: path.join(root, "another.json"),
    }),
  );
});
