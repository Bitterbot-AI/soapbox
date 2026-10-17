import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, readFile, stat } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { createHash } from 'node:crypto';
import path from 'node:path';
import { connection, privateJson, readJson } from './common.mjs';
import { install, rollback } from './install.mjs';
import { enroll } from './enroll.mjs';
const c={version:1,role:'buyer',id:'test-buyer',chain_id:84532,address:`0x${'1'.repeat(40)}`,escrow:`0x${'2'.repeat(40)}`,escrow_code_sha256:'a'.repeat(64),url:'https://compute.example',rpc_url:'https://sepolia.base.org',token:'b'.repeat(64)};
test('enrollment rejects mainnet, cleartext remote credentials and embedded credentials',()=>{
 for(const patch of [{chain_id:8453},{url:'http://remote.example'},{url:'https://user:pass@example.com'},{role:'admin'},{address:`0x${'0'.repeat(40)}`}]) assert.throws(()=>connection({...c,...patch}));
 assert.equal(connection({...c,unwantedSecret:'drop-me'}).unwantedSecret,undefined);
});
test('scoped enrollment exports no coordinator or wallet secrets and rejects duplicate IDs',async()=>{
 const root=await mkdtemp(path.join(tmpdir(),'soapbox-enroll-'));
 await privateJson(path.join(root,'coordinator.json'),{payments:{...c,chain_id:84532,wallets:{buyer:c.address}},principals:[{id:'buyer',token:'coordinator-secret'}]});
 const out=path.join(root,'operator.json');
 await enroll({state:root,role:'operator',id:'test-worker',address:c.address,url:c.url,output:out});
 const value=await readJson(out);
 assert.equal(value.role,'operator');assert.equal(value.chain_id,84532);assert.equal(value.wallets,undefined);
 assert.ok(!(await readFile(out,'utf8')).includes('coordinator-secret'));
 assert.equal((await stat(out)).mode & 0o777,0o600);
 await assert.rejects(enroll({state:root,role:'operator',id:'test-worker',address:c.address,url:c.url,output:path.join(root,'another.json')}));
});
test('install preserves main wallet, validates release integrity, supports both roles and guarded rollback',async()=>{
 const root=await mkdtemp(path.join(tmpdir(),'soapbox-install-')), release=path.join(root,'release'),state=path.join(root,'bitterbot');
 const files={};
 for(const file of ['plugin/index.cjs','plugin/bitterbot.plugin.json','bin/soapbox','ui/index.html','setup/install.mjs','setup/common.mjs']) {
  const content=file.endsWith('.json')?'{}':'fixture';await mkdir(path.dirname(path.join(release,file)),{recursive:true});await writeFile(path.join(release,file),content);files[file]=createHash('sha256').update(content).digest('hex');
 }
 await privateJson(path.join(release,'release.json'),{version:'test',platform:'linux-x64',chain_id:84532,bitterbot_version:'1.7.0',files});
 await privateJson(path.join(state,'app/current/package.json'),{version:'1.7.0'});
 const original={tools:{wallet:{network:'base',walletStorePath:'/mainnet',dailySpendLimitUsd:5}},agents:{preserve:true},gateway:{auth:{token:'existing-gateway'}}};
 await privateJson(path.join(state,'bitterbot.json'),original);
 const enrollment=path.join(root,'connection.json');await privateJson(enrollment,c);
 const plan=await install({release,state,enrollment,role:'buyer'});assert.equal(plan.dry_run,true);assert.deepEqual(await readJson(path.join(state,'bitterbot.json')),original);
 const installed=await install({release,state,enrollment,role:'buyer',apply:true});
 const config=await readJson(path.join(state,'bitterbot.json'));assert.deepEqual(config.tools,original.tools);assert.deepEqual(config.agents,original.agents);assert.equal(config.plugins.entries['soapbox-compute'].config.mode,'participant');
 await rollback(state,installed.target);assert.deepEqual(await readJson(path.join(state,'bitterbot.json')),original);
 await privateJson(enrollment,{...c,role:'operator',id:'worker-1'});
 await assert.rejects(install({release,state,enrollment,role:'operator',apply:true}));
 const workerConfig=path.join(root,'worker.json');await privateJson(workerConfig,{sandbox:{image:`sha256:${'a'.repeat(64)}`,models:root,gpu_lock:path.join(root,'gpu.lock')},allow_unsafe_host:false});
 const operator=await install({release,state,enrollment,role:'operator',workerConfig,apply:true});
 assert.ok((await readJson(path.join(state,'bitterbot.json'))).plugins.entries['soapbox-compute'].config.binary);
 await privateJson(path.join(state,'bitterbot.json'),{...config,changed:true});await assert.rejects(rollback(state,operator.target));
 await writeFile(path.join(release,'bin/soapbox'),'tampered');await assert.rejects(install({release,state,enrollment,role:'operator',workerConfig}));
});
