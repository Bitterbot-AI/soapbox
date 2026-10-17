import assert from 'node:assert/strict';
import { createHash, randomUUID } from 'node:crypto';
import { cp, chmod, mkdir, readFile, realpath, lstat, access } from 'node:fs/promises';
import { homedir } from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { parseArgs } from 'node:util';
import { readJson, privateJson, connection } from './common.mjs';
const sha = b => createHash('sha256').update(b).digest('hex');
export async function verifyRelease(release) {
  const meta = await readJson(path.join(release,'release.json'));
  assert.equal(meta.chain_id,84532); assert.equal(meta.platform,'linux-x64');
  assert.ok(meta.files && Object.keys(meta.files).length > 5);
  const root = await realpath(release);
  for (const [file, expected] of Object.entries(meta.files)) {
    assert.ok(!path.isAbsolute(file) && !file.split(/[\\/]/).some(p => p === '..'));
    const target = path.join(root,file);
    assert.ok((await realpath(target)).startsWith(root+path.sep));
    assert.ok((await lstat(target)).isFile());
    assert.equal(sha(await readFile(target)),expected,`Release integrity check failed: ${file}`);
  }
  for (const f of ['plugin/index.cjs','bin/soapbox','ui/index.html','setup/install.mjs']) assert.ok(meta.files[f],`Missing release asset ${f}`);
  return meta;
}
export async function install({ release, state, enrollment, role, workerConfig, apply = false }) {
  assert.equal(process.platform,'linux','Run in Ubuntu 24.04 or WSL2 Ubuntu 24.04');
  assert.equal(process.arch,'x64');
  assert.ok(Number(process.versions.node.split('.')[0]) >= 22,'Node 22 or later required');
  const meta = await verifyRelease(release);
  const c = connection(await readJson(enrollment));
  assert.equal(c.role,role,'Enrollment must match selected role');
  const configFile = path.join(state,'bitterbot.json');
  const before = await readFile(configFile,'utf8');
  const config = JSON.parse(before);
  assert.equal((await readJson(path.join(state,'app/current/package.json'))).version,meta.bitterbot_version,'Desktop UI must match the installed Bitterbot version');
  let worker;
  if (role === 'operator') {
    assert.ok(workerConfig,'Operator setup needs --worker-config from setup-operator.mjs');
    worker = await readJson(workerConfig);
    assert.ok(worker.sandbox && worker.allow_unsafe_host === false,'Isolated worker required');
    assert.match(worker.sandbox.image,/^sha256:[a-f0-9]{64}$/);
    assert.ok(path.isAbsolute(worker.sandbox.models) && path.isAbsolute(worker.sandbox.gpu_lock));
    await access(worker.sandbox.models);
  }
  const plan = { role, id:c.id, chain_id:84532, payout_or_buyer_address:c.address, bitterbot_version:meta.bitterbot_version, main_wallet_changed:false };
  if (!apply) return { ...plan, dry_run:true };
  const target = path.join(state,'compute',`soapbox-${meta.version}-${randomUUID()}`);
  await mkdir(target,{recursive:true,mode:0o700});
  for (const dir of ['plugin','ui','bin']) await cp(path.join(release,dir),path.join(target,dir),{recursive:true});
  await chmod(path.join(target,'bin/soapbox'),0o755);
  await privateJson(path.join(target,'connection.json'),c);
  if (worker) await privateJson(path.join(target,'worker.json'),worker);
  config.plugins ??= {}; config.plugins.load ??= {}; config.plugins.entries ??= {};
  const keep = [];
  for (const entry of config.plugins.load.paths ?? []) {
    const manifest = await readJson(path.join(entry,'bitterbot.plugin.json')).catch(() => null);
    if (manifest?.id !== 'soapbox-compute') keep.push(entry);
  }
  config.plugins.load.paths = [...keep,path.join(target,'plugin')];
  config.plugins.entries['soapbox-compute'] = { enabled:true,config:{mode:'participant',stateDirectory:target,...(worker ? {binary:path.join(target,'bin/soapbox')} : {})} };
  if (Array.isArray(config.plugins.allow) && !config.plugins.allow.includes('soapbox-compute')) config.plugins.allow.push('soapbox-compute');
  config.gateway ??= {}; config.gateway.controlUi = { ...config.gateway.controlUi,enabled:true,root:path.join(target,'ui') };
  assert.deepEqual(config.tools?.wallet,JSON.parse(before).tools?.wallet);
  await privateJson(path.join(target,'before-config.json'),JSON.parse(before));
  assert.equal(await readFile(configFile,'utf8'),before,'Configuration changed during install; retry');
  await privateJson(configFile,config);
  await privateJson(path.join(target,'installed.json'),{...plan,config_sha256:sha(await readFile(configFile)),target});
  return {...plan,target,installed:true};
}
export async function rollback(state,target) {
  const info = await readJson(path.join(target,'installed.json'));
  const file = path.join(state,'bitterbot.json');
  assert.equal(sha(await readFile(file)),info.config_sha256,'Configuration changed since install; merge the backup manually');
  await privateJson(file,await readJson(path.join(target,'before-config.json')));
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const { values:v } = parseArgs({ options:{ role:{type:'string'},connection:{type:'string'},'worker-config':{type:'string'},state:{type:'string',default:path.join(homedir(),'.bitterbot')},apply:{type:'boolean',default:false},rollback:{type:'string'} } });
  if(v.rollback) { await rollback(v.state,v.rollback); console.log('Original configuration restored. Restart Bitterbot.'); }
  else {
    const result = await install({release:path.resolve(import.meta.dirname,'..'),state:v.state,enrollment:v.connection,role:v.role,workerConfig:v['worker-config'],apply:v.apply});
    console.log(JSON.stringify(result,null,2));
    console.log(v.apply ? 'Installed. Restart Bitterbot, open Compute, and verify the participant ID and Base Sepolia address.' : 'Preflight passed. Repeat with --apply to install.');
  }
}
