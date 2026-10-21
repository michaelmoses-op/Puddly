#!/usr/bin/env node
/**
 * Puddly action sidecar — JSONL stdin/stdout protocol v2 (ADR-031 / Step 13).
 * Nested `op: "db"` for action `ctx.db` helpers (ADR-046 / Step 28).
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
 * until stdin closes (long-lived worker). Oneshot hosts close stdin after the final
 * response (stdin stays open during nested `db` round-trips).
 *
 * Nested DB (ADR-046): while handling `run`, the sidecar may write
 *   { v, id, op: "db", method, resource, input }
 * and the host replies with a normal { v, id, ok, data|error } line on stdin.
 *
 * Frozen error codes (must match Rust `IpcErrorCode`):
 *   protocol_mismatch, unknown_op, bad_request, not_found,
 *   import_failed, no_run_export, run_failed, sidecar_not_configured,
 *   unauthorized, forbidden, db_failed
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
  unauthorized: "unauthorized",
  forbidden: "forbidden",
  db_failed: "db_failed",
});

/** Incremented on every handled request (proves process reuse in tests). */
let handledCount = 0;

/** @type {Map<string, { resolve: (v: unknown) => void, reject: (e: Error) => void }>} */
const pendingDb = new Map();
let nestedSeq = 0;

function respond(id, obj) {
  process.stdout.write(JSON.stringify({ id, v: PROTOCOL_VERSION, ...obj }) + "\n");
}

function fail(id, code, message) {
  respond(id ?? 0, {
    ok: false,
    error: { code, message },
  });
}

/**
 * @param {string} method
 * @param {string} resource
 * @param {unknown} input
 */
function dbCall(method, resource, input) {
  nestedSeq += 1;
  const id = `db-${nestedSeq}`;
  return new Promise((resolve, reject) => {
    pendingDb.set(String(id), { resolve, reject });
    process.stdout.write(
      JSON.stringify({
        v: PROTOCOL_VERSION,
        id,
        op: "db",
        method,
        resource,
        input: input ?? {},
      }) + "\n",
    );
  });
}

function makeCtx(user) {
  return {
    user: user ?? null,
    db: {
      get: (resource, input) => dbCall("get", resource, input),
      list: (resource, input) => dbCall("list", resource, input ?? {}),
      create: (resource, input) => dbCall("create", resource, input),
      update: (resource, input) => dbCall("update", resource, input),
      delete: (resource, input) => dbCall("delete", resource, input),
    },
  };
}

/**
 * @param {Record<string, unknown>} req
 */
async function handleHostRequest(req) {
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

  const ctx = makeCtx(req.user ?? null);
  try {
    const data = await mod.run(req.input ?? {}, ctx);
    handledCount += 1;
    respond(id, {
      ok: true,
      data: data === undefined ? {} : data,
    });
  } catch (e) {
    // Nested ctx.db rejects as `code: message` — re-emit known wire codes
    // so the host can map them to HTTP (ADR-049) instead of always run_failed.
    fail(id, ...failFromThrown(e));
  }
}

/**
 * @param {unknown} e
 * @returns {[string, string]}
 */
function failFromThrown(e) {
  const message = e?.message ?? String(e);
  const m = /^([a-z_]+):\s([\s\S]*)$/.exec(message);
  if (m && Object.prototype.hasOwnProperty.call(IPC_ERROR_CODES, m[1])) {
    return [m[1], m[2]];
  }
  return [IPC_ERROR_CODES.run_failed, message];
}

const rl = createInterface({ input: process.stdin, crlfDelay: Infinity });

/** Host `run`/`ping` queue — must not block demux of nested db replies. */
const hostQueue = [];
let hostBusy = false;

async function pumpHost() {
  if (hostBusy) return;
  hostBusy = true;
  try {
    while (hostQueue.length > 0) {
      const req = hostQueue.shift();
      await handleHostRequest(req);
    }
  } finally {
    hostBusy = false;
  }
}

rl.on("line", (line) => {
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

  const idKey = String(req.id);
  if (pendingDb.has(idKey)) {
    const { resolve, reject } = pendingDb.get(idKey);
    pendingDb.delete(idKey);
    if (req.ok) {
      resolve(req.data ?? {});
    } else {
      const code = req.error?.code ?? IPC_ERROR_CODES.db_failed;
      const message = req.error?.message ?? "db failed";
      reject(new Error(`${code}: ${message}`));
    }
    return;
  }

  hostQueue.push(req);
  pumpHost().catch((e) => {
    fail(req.id ?? 0, IPC_ERROR_CODES.run_failed, e?.message ?? String(e));
  });
});

rl.on("close", () => {
  // stdin EOF — exit after in-flight work finishes.
  const finish = () => {
    process.exitCode = process.exitCode ?? 0;
  };
  if (hostBusy || hostQueue.length > 0) {
    const t = setInterval(() => {
      if (!hostBusy && hostQueue.length === 0) {
        clearInterval(t);
        finish();
      }
    }, 10);
  } else {
    finish();
  }
});
