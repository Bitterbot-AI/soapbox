import { createHash } from "node:crypto";
import fs from "node:fs/promises";
import {
  createPublicClient,
  createWalletClient,
  http,
  encodeFunctionData,
  encodeDeployData,
} from "viem";
import { privateKeyToAccount } from "viem/accounts";
import { compile } from "./compile.mjs";
import { createRelay } from "./relay.mjs";
export async function createPilot(testnet) {
  const artifacts = await compile();
  let server, wallets;
  if (!testnet) {
    const ganache = (await import("ganache")).default;
    server = ganache.server({
      logging: { quiet: true },
      chain: { chainId: 31337 },
      wallet: { deterministic: true },
    });
    await server.listen(0, "127.0.0.1");
    const transport = http(`http://127.0.0.1:${server.address().port}`);
    wallets = Object.values(server.provider.getInitialAccounts())
      .slice(0, 3)
      .map((a) => createWalletClient({ account: privateKeyToAccount(a.secretKey), transport }));
  }
  const rpcUrl = testnet?.rpcUrl ?? `http://127.0.0.1:${server.address().port}`;
  const client = createPublicClient({ transport: http(rpcUrl) });
  const chainId = testnet ? 84532 : 31337;
  if ((await client.getChainId()) !== chainId) throw Error("Wrong chain");
  const accounts =
    testnet?.accounts ??
    Object.fromEntries(
      ["buyer", "operator", "settler"].map((role, i) => [role, wallets[i].account.address]),
    );
  const records = [];
  const send = async (role, tx) => {
    const hash = testnet
      ? await testnet.send(role, tx)
      : await wallets[["buyer", "operator", "settler"].indexOf(role)].sendTransaction({
          ...tx,
          chain: null,
        });
    const receipt = await client.waitForTransactionReceipt({
      hash,
      confirmations: testnet ? 2 : 1,
    });
    if (receipt.status !== "success") throw Error("Pilot transaction reverted");
    records.push({ role: testnet && !tx.to ? "deployer" : role, transaction_hash: hash });
    return receipt;
  };
  let token = "0x036CbD53842c5426634e7929541eC2318f3dCF7e";
  if (!testnet)
    token = (await send("buyer", { data: artifacts.TestUSDC.bytecode })).contractAddress;
  const escrow =
    testnet?.escrow ??
    (
      await send("buyer", {
        data: encodeDeployData({ ...artifacts.SoapboxEscrow, args: [token, accounts.settler] }),
      })
    ).contractAddress;
  const code = await client.getCode({ address: escrow });
  const config = {
    rpc_url: rpcUrl,
    chain_id: chainId,
    token,
    escrow,
    escrow_code_sha256: createHash("sha256")
      .update(Buffer.from(code.slice(2), "hex"))
      .digest("hex"),
    microusdc_per_item: 1000,
    confirmations: testnet ? 2 : 1,
    wallets: { buyer: accounts.buyer, worker: accounts.operator },
  };
  if (testnet?.escrow) {
    if (config.escrow_code_sha256 !== testnet.escrowCodeHash)
      throw Error("Reused escrow bytecode mismatch");
    const tokenOnChain = await client.readContract({
      address: escrow,
      abi: artifacts.SoapboxEscrow.abi,
      functionName: "token",
    });
    const settlerOnChain = await client.readContract({
      address: escrow,
      abi: artifacts.SoapboxEscrow.abi,
      functionName: "coordinator",
    });
    if (
      tokenOnChain.toLowerCase() !== token.toLowerCase() ||
      settlerOnChain.toLowerCase() !== accounts.settler.toLowerCase()
    )
      throw Error("Reused escrow configuration mismatch");
  }
  const tokenAbi = artifacts.TestUSDC.abi;
  if (!testnet)
    await send("buyer", {
      to: token,
      data: encodeFunctionData({
        abi: tokenAbi,
        functionName: "mint",
        args: [accounts.buyer, 1000000n],
      }),
    });
  const balance = (address) =>
    client.readContract({
      address: token,
      abi: tokenAbi,
      functionName: "balanceOf",
      args: [address],
    });
  const initial = await balance(accounts.operator);
  return {
    config,
    accounts,
    records,
    initial: initial.toString(),
    fund: async (payment) => {
      await send("buyer", {
        to: token,
        data: encodeFunctionData({
          abi: tokenAbi,
          functionName: "approve",
          args: [escrow, BigInt(payment.amount_microusdc)],
        }),
      });
      await send("buyer", {
        to: escrow,
        data: encodeFunctionData({
          abi: artifacts.SoapboxEscrow.abi,
          functionName: "fund",
          args: [
            payment.key,
            payment.operator,
            BigInt(payment.amount_microusdc),
            BigInt(payment.expires),
          ],
        }),
      });
    },
    balance: async () => String(await balance(accounts.operator)),
    relay: async (url, token, crashAfterPayout = false) =>
      createRelay({
        config,
        url,
        token,
        signer: accounts.settler,
        sendTransaction: async (tx) => (await send("settler", tx)).transactionHash,
        onTransaction: (record) => {
          records.push(record);
          if (crashAfterPayout && record.action === "settle")
            throw Error("injected crash after confirmed payout");
        },
      }),
    close: async () => {
      if (server) await server.close();
    },
  };
}
