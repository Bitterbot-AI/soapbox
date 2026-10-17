import { createHash } from "node:crypto";
import fs from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { createPublicClient, createWalletClient, http, parseAbi, encodeFunctionData } from "viem";
import { privateKeyToAccount } from "viem/accounts";
const abi = parseAbi([
  "function token() view returns (address)",
  "function coordinator() view returns (address)",
  "function jobs(bytes32) view returns (address,address,uint256,uint256,uint256,bytes32)",
  "function settle(bytes32,bytes32)",
  "function refund(bytes32)",
  "function voidUnfunded(bytes32)",
]);
export async function createRelay({
  config,
  url,
  token,
  key,
  signer,
  sendTransaction,
  onTransaction = console.log,
}) {
  if (![31337, 84532].includes(config.chain_id))
    throw Error("Only local EVM and Base Sepolia enabled");
  const endpoint = new URL(url);
  if (
    endpoint.username ||
    endpoint.password ||
    endpoint.pathname !== "/" ||
    !(
      endpoint.protocol === "https:" ||
      (endpoint.protocol === "http:" && endpoint.hostname === "127.0.0.1")
    )
  )
    throw Error("Invalid coordinator URL");
  const transport = http(config.rpc_url),
    publicClient = createPublicClient({ transport });
  const wallet = key ? createWalletClient({ account: privateKeyToAccount(key), transport }) : null;
  const account = wallet?.account ?? signer;
  if (!account) throw Error("Settlement signer required");
  if ((await publicClient.getChainId()) !== config.chain_id) throw Error("Wrong chain");
  const code = await publicClient.getCode({ address: config.escrow });
  if (
    !code ||
    createHash("sha256")
      .update(Buffer.from(code.slice(2), "hex"))
      .digest("hex") !== config.escrow_code_sha256
  )
    throw Error("Escrow bytecode mismatch");
  if (
    (
      await publicClient.readContract({ address: config.escrow, abi, functionName: "coordinator" })
    ).toLowerCase() !== (typeof account === "string" ? account : account.address).toLowerCase()
  )
    throw Error("Wrong settlement signer");
  if (
    (
      await publicClient.readContract({ address: config.escrow, abi, functionName: "token" })
    ).toLowerCase() !== config.token.toLowerCase()
  )
    throw Error("Wrong token");
  if (
    config.chain_id === 84532 &&
    config.token.toLowerCase() !== "0x036cbd53842c5426634e7929541ec2318f3dcf7e".toLowerCase()
  )
    throw Error("Requires Circle Base Sepolia USDC");
  const api = async (path, method = "GET") => {
    const r = await fetch(new URL(path, url), {
      method,
      headers: { Authorization: `Bearer ${token}` },
      redirect: "error",
      signal: AbortSignal.timeout(20000),
    });
    if (!r.ok) throw Error(`Soapbox ${path}: ${r.status}`);
    return await r.json();
  };
  return async () => {
    const jobs = await api("/v1/settlements");
    const errors = [];
    for (const job of jobs) {
      try {
        const p = job.payment;
        if (
          p.chain_id !== config.chain_id ||
          p.escrow.toLowerCase() !== config.escrow.toLowerCase()
        )
          throw Error("Quote network mismatch");
        const state = await publicClient.readContract({
          address: config.escrow,
          abi,
          functionName: "jobs",
          args: [p.key],
        });
        if (
          state[4] === 1n &&
          (state[0].toLowerCase() !== p.buyer.toLowerCase() ||
            state[1].toLowerCase() !== p.operator.toLowerCase() ||
            state[2] !== BigInt(p.amount_microusdc) ||
            state[3] !== BigInt(p.expires))
        )
          throw Error("Escrow quote mismatch");
        let functionName, args;
        if (job.status === "settlement_pending" && state[4] === 1n) {
          functionName = "settle";
          args = [p.key, `0x${job.result_sha256}`];
        }
        if (job.status === "refund_pending" && [0n, 1n].includes(state[4])) {
          functionName = state[4] === 0n ? "voidUnfunded" : "refund";
          args = [p.key];
        }
        if (state[4] === 1n && (await publicClient.getBlock()).timestamp >= state[3]) {
          functionName = "refund";
          args = [p.key];
        }
        if (functionName) {
          const { request } = await publicClient.simulateContract({
            address: config.escrow,
            abi,
            functionName,
            args,
            account,
          });
          const hash = wallet
            ? await wallet.writeContract({ ...request, chain: null })
            : await sendTransaction({
                to: config.escrow,
                data: encodeFunctionData({ abi, functionName, args }),
                value: 0n,
              });
          onTransaction({ job: job.id, action: functionName, transaction_hash: hash });
          const receipt = await publicClient.waitForTransactionReceipt({
            hash,
            confirmations: config.confirmations,
          });
          if (receipt.status !== "success") throw Error("Escrow transaction reverted");
        }
        await api(`/v1/jobs/${job.id}/payment`, "POST");
      } catch (error) {
        errors.push(`${job.id}: ${error.message}`);
      }
    }
    if (errors.length) throw Error(errors.join("; "));
  };
}
if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const config = JSON.parse(await fs.readFile(process.argv[2], "utf8")).payments;
  const tick = await createRelay({
    config,
    url: process.env.SOAPBOX_URL ?? "http://127.0.0.1:8787",
    token: process.env.SOAPBOX_ADMIN_TOKEN,
    key: process.env.SOAPBOX_SETTLER_PRIVATE_KEY,
  });
  do {
    try {
      await tick();
    } catch (e) {
      console.error(e.message);
    }
    if (process.argv.includes("--once")) break;
    await new Promise((r) => setTimeout(r, 3000));
  } while (true);
}
