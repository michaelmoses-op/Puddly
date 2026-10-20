#!/usr/bin/env node
/**
 * Puddly action sidecar — JSONL stdin/stdout protocol v2 (ADR-031 / Step 13).
 *
 * Usage:
 *   node --experimental-strip-types tools/puddly-action-sidecar.mjs
 *   # or via IpcActionRunner::node_sidecar / generated `.puddly/generated/action-sidecar.mjs`
 *
 * Env:
 *   PUDDLY_ACTION_SIDECAR — optional override used by the Rust host
 *     (space-separated argv, e.g. "node --experimental-strip-types /abs/path/to/this.mjs")
 *
 * Protocol v2: one JSON request line per call on stdin → one JSON response line on
 * stdout, correlated by `id` (string or u64). The process stays alive across lines
 * until stdin closes (long-lived worker). Oneshot hosts close stdin after one line.
 *
 * Frozen error codes (must match Rust `IpcErrorCode`):
 *   protocol_mismatch, unknown_op, bad_request, not_found,
 *   import_failed, no_run_export, run_failed, sidecar_not_configured
 */

import { createInterface } from "node:readline";
import { pathToFileURL } from "node:url";
import { isAbsolute, resolve } from "node:path";
import { access } from "node:fs/promises";
import { constants as fsConstants } from "node:fs";

const PROTOCOL_VERSION = 2;

/** @type {Readonly<Record<string, string>>} */
export const IPC_ERROR_CODES = Object.freeze({
  protocol_mismatch: "protocol_mismatch",
  unknown_op: "unknown_op",
  bad_request: "bad_request",
  not_found: "not_found",
  import_failed: "import_failed",
  no_run_export: "no_run_export",
  run_failed: "run_failed",
  sidecar_not_configured: "sidecar_not_configured",
});

/** Incremented on every handled request (proves process reuse in tests). */
let handledCount = 0;

function respond(id, obj) {
  process.stdout.write(JSON.stringify({ id, v: PROTOCOL_VERSION, ...obj }) + "\n");
}

function fail(id, code, message) {
  respond(id ?? 0, {
    ok: false,
    error: { code, message },
  });
}

async function handle(req) {
  const id = req.id;
  if (id === undefined || id === null) {
    fail(0, IPC_ERROR_CODES.bad_request, "id required");
    return;
  }

  if (req.v !== PROTOCOL_VERSION) {
    fail(
      id,
      IPC_ERROR_CODES.protocol_mismatch,
      `got v${req.v}, expected v${PROTOCOL_VERSION}`,
    );
    return;
  }

  // Health / reuse probe used by Step 13 tests.
  if (req.op === "ping") {
    handledCount += 1;
    respond(id, {
      ok: true,
      data: { pid: process.pid, seq: handledCount },
    });
    return;
  }

  if (req.op !== "run") {
    fail(id, IPC_ERROR_CODES.unknown_op, `unsupported op \`${req.op}\``);
    return;
  }
  if (!req.sourcePath || typeof req.sourcePath !== "string") {
    fail(id, IPC_ERROR_CODES.bad_request, "sourcePath required");
    return;
  }

  const abs = isAbsolute(req.sourcePath)
    ? req.sourcePath
    : resolve(process.cwd(), req.sourcePath);

  try {
    await access(abs, fsConstants.R_OK);
  } catch {
    fail(id, IPC_ERROR_CODES.not_found, `action file not readable: ${abs}`);
    return;
  }

  let mod;
  try {
    mod = await import(pathToFileURL(abs).href);
  } catch (e) {
    fail(id, IPC_ERROR_CODES.import_failed, e?.message ?? String(e));
    return;
  }

  if (typeof mod.run !== "function") {
    fail(
      id,
      IPC_ERROR_CODES.no_run_export,
      `module ${abs} has no export async function run`,
    );
    return;
  }

  const ctx = { user: req.user ?? null };
  try {
    const data = await mod.run(req.input ?? {}, ctx);
    handledCount += 1;
    respond(id, {
      ok: true,
      data: data === undefined ? {} : data,
    });
  } catch (e) {
    fail(id, IPC_ERROR_CODES.run_failed, e?.message ?? String(e));
  }
}

const rl = createInterface({ input: process.stdin, crlfDelay: Infinity });

/** Serialize handlers so overlapping readline events stay ordered. */
let chain = Promise.resolve();

rl.on("line", (line) => {
  chain = chain.then(async () => {
    const trimmed = line.trim();
    if (!trimmed) {
      fail(0, IPC_ERROR_CODES.bad_request, "empty request line");
      return;
    }
    let req;
    try {
      req = JSON.parse(trimmed);
    } catch (e) {
      fail(0, IPC_ERROR_CODES.bad_request, `invalid JSON: ${e?.message ?? e}`);
      return;
    }
    await handle(req);
  });
});

rl.on("close", () => {
  // stdin EOF — exit after in-flight work finishes.
  chain.finally(() => {
    process.exitCode = process.exitCode ?? 0;
  });
});
