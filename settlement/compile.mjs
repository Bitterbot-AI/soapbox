import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import solc from "solc";
const root = path.dirname(fileURLToPath(import.meta.url));
export async function compile() {
  const sources = Object.fromEntries(
    await Promise.all(
      ["SoapboxEscrow", "TestUSDC"].map(async (name) => [
        `${name}.sol`,
        { content: await fs.readFile(path.join(root, "../contracts", `${name}.sol`), "utf8") },
      ]),
    ),
  );
  const output = JSON.parse(
    solc.compile(
      JSON.stringify({
        language: "Solidity",
        sources,
        settings: {
          evmVersion: "paris",
          optimizer: { enabled: true, runs: 200 },
          outputSelection: {
            "*": { "*": ["abi", "evm.bytecode.object", "evm.deployedBytecode.object"] },
          },
        },
      }),
    ),
  );
  for (const e of output.errors ?? []) if (e.severity === "error") throw Error(e.formattedMessage);
  const artifacts = Object.fromEntries(
    ["SoapboxEscrow", "TestUSDC"].map((name) => {
      const c = output.contracts[`${name}.sol`][name];
      return [name, { abi: c.abi, bytecode: `0x${c.evm.bytecode.object}` }];
    }),
  );
  await fs.mkdir(path.join(root, "build"), { recursive: true });
  for (const [name, a] of Object.entries(artifacts))
    await fs.writeFile(path.join(root, "build", `${name}.json`), JSON.stringify(a, null, 2));
  return artifacts;
}
if (process.argv[1] === fileURLToPath(import.meta.url)) await compile();
