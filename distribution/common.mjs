import assert from "node:assert/strict";
import { readFile, writeFile, rename, mkdir } from "node:fs/promises";
import path from "node:path";
export const readJson = async (file) => JSON.parse(await readFile(file, "utf8"));
export const address = (value) =>
  typeof value === "string" && /^0x[\da-f]{40}$/i.test(value) && !/^0x0{40}$/i.test(value);
export function url(value) {
  const u = new URL(value);
  assert.ok(
    (u.protocol === "https:" ||
      (u.protocol === "http:" && ["localhost", "127.0.0.1", "[::1]"].includes(u.hostname))) &&
      !u.username &&
      !u.password &&
      !u.search &&
      !u.hash &&
      u.pathname === "/",
    "Use a root HTTPS URL or an SSH tunnel on loopback",
  );
  return value;
}
export function connection(c) {
  assert.ok(c?.version === 1 && ["buyer", "operator"].includes(c.role), "Invalid participant role");
  assert.match(c.id, /^[a-zA-Z0-9_-]{1,128}$/);
  assert.equal(c.chain_id, 84532, "Only Base Sepolia is supported");
  assert.ok(address(c.address) && address(c.escrow), "Invalid public address");
  assert.match(c.token, /^[\da-f]{64}$/i);
  assert.match(c.escrow_code_sha256, /^[\da-f]{64}$/i);
  url(c.url);
  const rpc = new URL(c.rpc_url);
  assert.ok(rpc.protocol === "https:" && !rpc.username && !rpc.password && !rpc.hash);
  return Object.fromEntries(
    [
      "version",
      "role",
      "id",
      "url",
      "token",
      "address",
      "chain_id",
      "escrow",
      "escrow_code_sha256",
      "rpc_url",
    ].map((k) => [k, c[k]]),
  );
}
export async function privateJson(file, data) {
  await mkdir(path.dirname(file), { recursive: true, mode: 0o700 });
  const tmp = `${file}.next-${process.pid}`;
  await writeFile(tmp, `${JSON.stringify(data, null, 2)}\n`, { mode: 0o600, flag: "wx" });
  await rename(tmp, file);
}
