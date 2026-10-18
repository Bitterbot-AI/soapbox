import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { copyFile, mkdir, readFile, realpath, rename, writeFile } from 'node:fs/promises';
import path from 'node:path';
const sha = (bytes) => createHash('sha256').update(bytes).digest('hex');
const names = ['entry.js', 'babel.cjs'];
export async function stageGateway(state, target) {
  const app = await realpath(path.join(state, 'app/current'));
  const appRoot = await realpath(path.join(state, 'app'));
  assert.ok(app.startsWith(appRoot + path.sep), 'Gateway must be inside this installation');
  const files = [];
  await mkdir(path.join(target, 'gateway-before'));
  for (const name of names) {
    const destination = path.join(app, 'dist', name);
    assert.equal(await realpath(destination), destination, 'Gateway files must not be symlinks');
    const backup = path.join(target, 'gateway-before', name);
    await copyFile(destination, backup);
    files.push({ name, destination, before: sha(await readFile(backup)), after: sha(await readFile(path.join(target, 'gateway', name))) });
  }
  return { app, files };
}
async function swap(file, bytes) {
  const temp = file + '.soapbox-next';
  await writeFile(temp, bytes, { mode: 0o644 });
  await rename(temp, file);
}
export async function validateGateway(plan, target, restore = false) {
  for (const f of plan.files) {
    assert.ok(names.includes(f.name) && f.destination === path.join(plan.app, 'dist', f.name));
    const current = sha(await readFile(f.destination));
    assert.ok(current === f.before || current === f.after, 'Gateway changed outside this installer; automatic rollback is unavailable');
    const source = path.join(target, restore ? 'gateway-before' : 'gateway', f.name);
    assert.equal(sha(await readFile(source)), restore ? f.before : f.after, 'Gateway backup or update is damaged');
  }
}
export async function applyGateway(plan, target, restore = false) {
  await validateGateway(plan, target, restore);
  const changed = [];
  try {
    for (const f of plan.files) {
      const bytes = await readFile(f.destination);
      changed.push({ destination: f.destination, bytes });
      await swap(f.destination, await readFile(path.join(target, restore ? 'gateway-before' : 'gateway', f.name)));
    }
  } catch (error) {
    for (const f of changed.reverse()) await swap(f.destination, f.bytes);
    throw error;
  }
}
