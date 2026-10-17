import ganache from "ganache";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { createServer } from "node:http";
import test from "node:test";
import { createPublicClient, createWalletClient, http, keccak256, toHex } from "viem";
import { privateKeyToAccount } from "viem/accounts";
import { compile } from "./compile.mjs";
import { createRelay } from "./relay.mjs";
test("fund, fixed-payee payout, retries, authorization, expiry refund and failure atomicity", async () => {
  const server = ganache.server({
    logging: { quiet: true },
    chain: { chainId: 31337 },
    wallet: { deterministic: true },
  });
  await server.listen(0, "127.0.0.1");
  try {
    const transport = http(`http://127.0.0.1:${server.address().port}`);
    const publicClient = createPublicClient({ transport });
    const accounts = Object.values(server.provider.getInitialAccounts()).map((a) =>
      privateKeyToAccount(a.secretKey),
    );
    const wallets = accounts
      .slice(0, 4)
      .map((account) => createWalletClient({ account, transport }));
    const [buyer, coordinator, operator, attacker] = wallets;
    const artifacts = await compile();
    const send = async (wallet, args) => {
      const hash = await wallet.writeContract({ ...args, chain: null });
      const r = await publicClient.waitForTransactionReceipt({ hash });
      assert.equal(r.status, "success");
      return r;
    };
    const deploy = async (name, args = []) => {
      const hash = await buyer.deployContract({ ...artifacts[name], args, chain: null });
      return (await publicClient.waitForTransactionReceipt({ hash })).contractAddress;
    };
    const token = await deploy("TestUSDC");
    const escrow = await deploy("SoapboxEscrow", [token, coordinator.account.address]);
    const t = { address: token, abi: artifacts.TestUSDC.abi };
    const e = { address: escrow, abi: artifacts.SoapboxEscrow.abi };
    const balance = (address) =>
      publicClient.readContract({ ...t, functionName: "balanceOf", args: [address] });
    const state = (id) => publicClient.readContract({ ...e, functionName: "jobs", args: [id] });
    await send(buyer, { ...t, functionName: "mint", args: [buyer.account.address, 1000000n] });
    await send(buyer, { ...t, functionName: "approve", args: [escrow, 1000000n] });
    const expires = (await publicClient.getBlock()).timestamp + 3600n;
    const id = keccak256(toHex("paid-compute")),
      result = keccak256(toHex("verified-output"));
    await send(buyer, {
      ...e,
      functionName: "fund",
      args: [id, operator.account.address, 33000n, expires],
    });
    assert.equal(await balance(operator.account.address), 0n);
    await assert.rejects(send(attacker, { ...e, functionName: "settle", args: [id, result] }));
    await assert.rejects(send(buyer, { ...e, functionName: "refund", args: [id] }));
    await send(coordinator, { ...e, functionName: "settle", args: [id, result] });
    assert.equal(await balance(operator.account.address), 33000n);
    assert.equal((await state(id))[4], 2n);
    await assert.rejects(send(coordinator, { ...e, functionName: "settle", args: [id, result] }));
    await assert.rejects(send(coordinator, { ...e, functionName: "refund", args: [id] }));
    await assert.rejects(
      send(buyer, {
        ...e,
        functionName: "fund",
        args: [id, attacker.account.address, 33000n, expires],
      }),
    );
    assert.equal(await balance(operator.account.address), 33000n);
    const cancelled = keccak256(toHex("cancelled"));
    await send(buyer, {
      ...e,
      functionName: "fund",
      args: [cancelled, operator.account.address, 10000n, expires],
    });
    await send(coordinator, { ...e, functionName: "refund", args: [cancelled] });
    assert.equal((await state(cancelled))[4], 3n);
    const voided = keccak256(toHex("voided"));
    await assert.rejects(send(attacker, { ...e, functionName: "voidUnfunded", args: [voided] }));
    await send(coordinator, { ...e, functionName: "voidUnfunded", args: [voided] });
    await assert.rejects(
      send(buyer, {
        ...e,
        functionName: "fund",
        args: [voided, operator.account.address, 1000n, expires],
      }),
    );
    assert.equal((await state(voided))[4], 4n);
    const insufficient = keccak256(toHex("insufficient"));
    await assert.rejects(
      send(buyer, {
        ...e,
        functionName: "fund",
        args: [insufficient, operator.account.address, 2000000n, expires],
      }),
    );
    assert.equal((await state(insufficient))[4], 0n);
    assert.equal(await balance(escrow), 0n);
    const expired = keccak256(toHex("expired"));
    await send(buyer, {
      ...e,
      functionName: "fund",
      args: [expired, operator.account.address, 10000n, expires],
    });
    const relayExpired = keccak256(toHex("relay-expired"));
    await send(buyer, {
      ...e,
      functionName: "fund",
      args: [relayExpired, operator.account.address, 10000n, expires],
    });
    await server.provider.request({ method: "evm_increaseTime", params: [3601] });
    await server.provider.request({ method: "evm_mine", params: [] });
    await assert.rejects(
      send(coordinator, { ...e, functionName: "settle", args: [expired, result] }),
    );
    await send(attacker, { ...e, functionName: "refund", args: [expired] });
    const code = await publicClient.getCode({ address: escrow });
    const config = {
      chain_id: 31337,
      rpc_url: `http://127.0.0.1:${server.address().port}`,
      token,
      escrow,
      escrow_code_sha256: createHash("sha256")
        .update(Buffer.from(code.slice(2), "hex"))
        .digest("hex"),
      confirmations: 1,
    };
    let acknowledged = 0;
    const api = createServer((req, res) => {
      res.setHeader("Content-Type", "application/json");
      if (req.method === "GET")
        res.end(
          JSON.stringify([
            {
              id: "expired",
              status: "settlement_pending",
              result_sha256: result.slice(2),
              payment: {
                chain_id: 31337,
                escrow,
                key: relayExpired,
                buyer: buyer.account.address,
                operator: operator.account.address,
                amount_microusdc: 10000,
                expires: Number(expires),
              },
            },
          ]),
        );
      else {
        acknowledged++;
        res.end("{}");
      }
    });
    await new Promise((resolve) => api.listen(0, "127.0.0.1", resolve));
    try {
      const relay = await createRelay({
        config,
        url: `http://127.0.0.1:${api.address().port}`,
        token: "local-test",
        key: Object.values(server.provider.getInitialAccounts())[1].secretKey,
        onTransaction: () => {},
      });
      await relay();
      await relay();
      assert.equal((await state(relayExpired))[4], 3n);
      assert.equal(acknowledged, 2);
    } finally {
      await new Promise((resolve) => api.close(resolve));
    }
    assert.equal(await balance(buyer.account.address), 967000n);
    assert.equal(await balance(escrow), 0n);
  } finally {
    await server.close();
  }
});
