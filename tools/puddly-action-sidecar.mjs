#!/usr/bin/env node
/**
 * Puddly action sidecar — JSONL stdin/stdout protocol v1 (ADR / Step 9).
 *
 * Usage:
 *   node --experimental-strip-types tools/puddly-action-sidecar.mjs
 *   # or via IpcActionRunner::node_sidecar(workspace_root)
 *
 * Env:
 *   PUDDLY_ACTION_SIDECAR — optional override used by the Rust host
 *     (space-separated argv, e.g. "node --experimental-strip-types /abs/path/to/this.mjs")
 *
 * Protocol: one JSON request line on stdin → one JSON response line on stdout.
 * Loads `sourcePath` with dynamic import and calls `export async function run(input, ctx)`.
 */

import { createInterface } from "node:readline";
import { pathToFileURL } from "node:url";
import { isAbsolute, resolve } from "node:path";
import { access } from "node:fs/promises";
import { constants as fsConstants } from "node:fs";

const PROTOCOL_VERSION = 1;

function respond(obj) {
  process.stdout.write(JSON.stringify(obj) + "\n");
}

function fail(code, message) {
  respond({
    v: PROTOCOL_VERSION,
    ok: false,
    error: { code, message },
  });
}

async function handle(req) {
  if (req.v !== PROTOCOL_VERSION) {
    fail(
      "protocol_mismatch",
      `got v${req.v}, expected v${PROTOCOL_VERSION}`,
    );
    return;
  }
  if (req.op !== "run") {
    fail("unknown_op", `unsupported op \`${req.op}\``);
    return;
  }
  if (!req.sourcePath || typeof req.sourcePath !== "string") {
    fail("bad_request", "sourcePath required");
    return;
  }

  const abs = isAbsolute(req.sourcePath)
    ? req.sourcePath
    : resolve(process.cwd(), req.sourcePath);

  try {
    await access(abs, fsConstants.R_OK);
  } catch {
    fail("not_found", `action file not readable: ${abs}`);
    return;
  }

  let mod;
  try {
    mod = await import(pathToFileURL(abs).href);
  } catch (e) {
    fail("import_failed", e?.message ?? String(e));
    return;
  }

  if (typeof mod.run !== "function") {
    fail("no_run_export", `module ${abs} has no export async function run`);
    return;
  }

  const ctx = { user: req.user ?? null };
  try {
    const data = await mod.run(req.input ?? {}, ctx);
    respond({
      v: PROTOCOL_VERSION,
      ok: true,
      data: data === undefined ? {} : data,
    });
  } catch (e) {
    fail("run_failed", e?.message ?? String(e));
  }
}

const rl = createInterface({ input: process.stdin, crlfDelay: Infinity });
let handled = false;

rl.on("line", async (line) => {
  if (handled) return;
  handled = true;
  rl.close();
  const trimmed = line.trim();
  if (!trimmed) {
    fail("bad_request", "empty request line");
    process.exitCode = 1;
    return;
  }
  let req;
  try {
    req = JSON.parse(trimmed);
  } catch (e) {
    fail("bad_request", `invalid JSON: ${e?.message ?? e}`);
    process.exitCode = 1;
    return;
  }
  await handle(req);
});

rl.on("close", () => {
  if (!handled) {
    fail("bad_request", "no request line on stdin");
    process.exitCode = 1;
  }
});
