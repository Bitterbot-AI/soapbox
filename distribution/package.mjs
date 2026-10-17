/** Reproducible input selection: no local config, wallet, logs, model cache or databases. */
import assert from "node:assert/strict";
import runtime from "./runtime.json" with { type: "json" };
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, cp, readFile, writeFile, readdir, chmod, stat } from "node:fs/promises";
import path from "node:path";
import { parseArgs } from "node:util";
const { values: v } = parseArgs({
  options: Object.fromEntries(
    [
      "desktop",
      "binary",
      "output",
      "desktop-commit",
      "runtime",
      "windows-launcher",
      "linux-launcher",
    ].map((k) => [k, { type: "string" }]),
  ),
});
const root = path.resolve(import.meta.dirname, "..");
const version = "0.3.0-rc.4",
  name = `soapbox-${version}-linux-x64`,
  destination = path.join(v.output, name);
assert.ok(v.desktop && v.binary && /^[a-f0-9]{40}$/.test(v["desktop-commit"] ?? ""));
await mkdir(v.output, { recursive: true });
await mkdir(destination);
const build = JSON.parse(await readFile(path.join(v.desktop, "build.json"), "utf8"));
await mkdir(path.join(destination, "plugin"), { recursive: true });
for (const file of ["index.cjs", "buyer-wallet.cjs", "package.json", "bitterbot.plugin.json"])
  await cp(path.join(v.desktop, "plugin", file), path.join(destination, "plugin", file));
await cp(path.join(v.desktop, "ui"), path.join(destination, "ui"), { recursive: true });
await mkdir(path.join(destination, "bin"), { recursive: true });
await cp(v.binary, path.join(destination, "bin/soapbox"));
await chmod(path.join(destination, "bin/soapbox"), 0o755);
assert.equal(
  createHash("sha256")
    .update(await readFile(v.runtime))
    .digest("hex"),
  runtime.sha256,
  "Use the approved runtime archive",
);
await mkdir(path.join(destination, "runtime"));
await cp(v.runtime, path.join(destination, "runtime", runtime.filename));
await cp(v["windows-launcher"], path.join(destination, "Soapbox-Setup.exe"));
await cp(v["linux-launcher"], path.join(destination, "Soapbox-Setup"));
await chmod(path.join(destination, "Soapbox-Setup"), 0o755);
await cp(path.join(root, "distribution"), path.join(destination, "setup"), {
  recursive: true,
  filter: (p) => !p.endsWith(".test.mjs") && !p.endsWith("package.mjs"),
});
await cp(path.join(root, "containers"), path.join(destination, "containers"), { recursive: true });
await cp(path.join(root, "docs/TEST-RELEASE.md"), path.join(destination, "README.md"));
await cp(path.join(root, "LICENSE"), path.join(destination, "LICENSE"));
await mkdir(path.join(destination, "provenance"), { recursive: true });
await cp(path.join(root, "Cargo.lock"), path.join(destination, "provenance/Cargo.lock"));
await cp(
  path.join(root, "settlement/package-lock.json"),
  path.join(destination, "provenance/settlement-package-lock.json"),
);
const files = {};
async function walk(dir) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) await walk(full);
    else if (entry.name !== "release.json") {
      assert.ok(entry.isFile());
      files[path.relative(destination, full).split(path.sep).join("/")] = createHash("sha256")
        .update(await readFile(full))
        .digest("hex");
    }
  }
}
await walk(destination);
await writeFile(
  path.join(destination, "release.json"),
  JSON.stringify(
    {
      version,
      installer_version: 2,
      runtime_version: runtime.version,
      platform: "linux-x64",
      os: "Ubuntu 24.04 / WSL2 Ubuntu 24.04",
      chain_id: 84532,
      bitterbot_version: build.version,
      soapbox_commit: execFileSync("git", ["rev-parse", "HEAD"], {
        cwd: root,
        encoding: "utf8",
      }).trim(),
      desktop_commit: v["desktop-commit"],
      files,
    },
    null,
    2,
  ),
);
execFileSync("tar", ["-czf", `${name}.tar.gz`, name], { cwd: v.output, stdio: "inherit" });
const archive = path.join(v.output, `${name}.tar.gz`);
const hash = createHash("sha256")
  .update(await readFile(archive))
  .digest("hex");
const zipName = `soapbox-${version}-windows-wsl.zip`;
execFileSync("python3", [
  "-c",
  "import pathlib,sys,zipfile; root=pathlib.Path(sys.argv[1]); z=zipfile.ZipFile(sys.argv[2],'w',zipfile.ZIP_DEFLATED); [z.write(p,p.relative_to(root.parent)) for p in sorted(root.rglob('*')) if p.is_file()]; z.close()",
  destination,
  path.join(v.output, zipName),
]);
const zipHash = createHash("sha256")
  .update(await readFile(path.join(v.output, zipName)))
  .digest("hex");
await cp(v.runtime, path.join(v.output, runtime.filename));
await writeFile(
  path.join(v.output, "SHA256SUMS"),
  `${hash}  ${name}.tar.gz\n${zipHash}  ${zipName}\n${runtime.sha256}  ${runtime.filename}\n`,
);
console.log(
  JSON.stringify(
    { archive, sha256: hash, bytes: (await stat(archive)).size, files: Object.keys(files).length },
    null,
    2,
  ),
);
