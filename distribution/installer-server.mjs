import { randomBytes, timingSafeEqual } from "node:crypto";
import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import { homedir } from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { parseArgs } from "node:util";
import { readJson } from "./common.mjs";
import { preflight, install, rollback, latestInstall } from "./install.mjs";

export async function createInstallerServer({ release, state, port = 0, facts }) {
  const secret = randomBytes(32).toString("hex");
  let origin,
    busy = false;
  const server = createServer(async (req, res) => {
    res.setHeader("Cache-Control", "no-store");
    res.setHeader("X-Content-Type-Options", "nosniff");
    res.setHeader("Referrer-Policy", "no-referrer");
    res.setHeader(
      "Content-Security-Policy",
      "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
    );
    const reply = (status, data) => {
      res.writeHead(status, { "Content-Type": "application/json" });
      res.end(JSON.stringify(data));
    };
    if (req.headers.host !== new URL(origin).host)
      return reply(403, { error: "Invalid local installer host" });
    const files = {
      "/": ["installer.html", "text/html"],
      "/installer.js": ["installer.js", "text/javascript"],
      "/installer.css": ["installer.css", "text/css"],
    };
    if (req.method === "GET" && files[req.url]) {
      const [file, type] = files[req.url];
      res.setHeader("Content-Type", type);
      return res.end(await readFile(path.join(import.meta.dirname, file)));
    }
    const token = Buffer.from(req.headers.authorization ?? ""),
      expected = Buffer.from("Bearer " + secret);
    if (
      token.length !== expected.length ||
      !timingSafeEqual(token, expected) ||
      (req.headers.origin && req.headers.origin !== origin)
    )
      return reply(403, { error: "Reopen the installer to authenticate this window" });
    try {
      if (req.method === "GET" && req.url === "/api/status") {
        const target = await latestInstall(state);
        const config = await readJson(path.join(state, "bitterbot.json")).catch(() => ({}));
        const port = Number(config.gateway?.port ?? 19001);
        return reply(200, {
          chain_id: 84532,
          rollback_available: Boolean(
            target && config.gateway?.controlUi?.root === path.join(target, "ui"),
          ),
          desktop_url:
            Number.isInteger(port) && port > 0 && port < 65536 ? `http://127.0.0.1:${port}/` : null,
        });
      }
      if (
        req.method !== "POST" ||
        !["/api/check", "/api/install", "/api/rollback", "/api/close"].includes(req.url)
      )
        return reply(404, { error: "Not found" });
      if (req.headers["content-type"] !== "application/json")
        return reply(415, { error: "JSON required" });
      if (busy) return reply(409, { error: "Wait for the current operation to finish" });
      let body = "";
      for await (const part of req) {
        body += part;
        if (body.length > 1024) return reply(413, { error: "Request too large" });
      }
      const data = JSON.parse(body || "{}");
      if (!["buyer", "operator", undefined].includes(data.role))
        return reply(400, { error: "Invalid role" });
      busy = true;
      try {
        if (req.url === "/api/check")
          return reply(200, await preflight({ release, state, role: data.role, facts }));
        if (req.url === "/api/install")
          return reply(200, await install({ release, state, role: data.role, apply: true, facts }));
        if (req.url === "/api/rollback") {
          const target = await latestInstall(state);
          if (!target) throw Error("No installation to roll back");
          return reply(200, await rollback(state, target));
        }
        reply(200, { closed: true });
        server.close();
        server.closeIdleConnections();
      } finally {
        busy = false;
      }
    } catch (e) {
      reply(400, {
        error: e instanceof SyntaxError ? "Invalid configuration or request JSON" : e.message,
      });
    }
  });
  server.requestTimeout = 30000;
  server.headersTimeout = 15000;
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(port, "127.0.0.1", resolve);
  });
  origin = `http://127.0.0.1:${server.address().port}`;
  const expiry = setTimeout(
    () => {
      server.close();
      server.closeAllConnections();
    },
    30 * 60 * 1000,
  );
  expiry.unref();
  server.on("close", () => clearTimeout(expiry));
  return { server, url: origin + "/#" + secret, origin, secret };
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const { values } = parseArgs({
    options: { state: { type: "string", default: path.join(homedir(), ".bitterbot") } },
  });
  const app = await createInstallerServer({
    release: path.resolve(import.meta.dirname, ".."),
    state: path.resolve(values.state),
  });
  console.log(app.url);
}
