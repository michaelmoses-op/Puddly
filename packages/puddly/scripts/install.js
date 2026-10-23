"use strict";

/**
 * postinstall: download the matching prebuilt `puddly` binary from GitHub Releases
 * into vendor/ (esbuild/swc pattern). Does **not** compile the Rust workspace.
 *
 * Env:
 *   PUDDLY_SKIP_DOWNLOAD=1   skip (use an already-present vendor/puddly, e.g. CI smoke)
 *   PUDDLY_BINARY=/path      copy that file into vendor/puddly
 *   PUDDLY_REPO=owner/repo   override GitHub repo
 *   PUDDLY_VERSION=0.1.0     override package version for the asset name
 */

const fs = require("fs");
const path = require("path");
const https = require("https");
const http = require("http");
const { execFileSync } = require("child_process");
const {
  detectPlatform,
  packageVersion,
  releaseTag,
  assetName,
  releaseDownloadUrl,
  DEFAULT_REPO,
} = require("./platform");

const ROOT = path.join(__dirname, "..");
const VENDOR = path.join(ROOT, "vendor");
const DEST = path.join(VENDOR, "puddly");

function log(msg) {
  console.log(`[puddly] ${msg}`);
}

function warn(msg) {
  console.warn(`[puddly] ${msg}`);
}

function download(url) {
  return new Promise((resolve, reject) => {
    const get = url.startsWith("https") ? https.get : http.get;
    const req = get(url, { headers: { "User-Agent": "puddly-npm-install" } }, (res) => {
      if (
        res.statusCode >= 300 &&
        res.statusCode < 400 &&
        res.headers.location
      ) {
        res.resume();
        download(res.headers.location).then(resolve, reject);
        return;
      }
      if (res.statusCode !== 200) {
        res.resume();
        reject(new Error(`HTTP ${res.statusCode} for ${url}`));
        return;
      }
      const chunks = [];
      res.on("data", (c) => chunks.push(c));
      res.on("end", () => resolve(Buffer.concat(chunks)));
      res.on("error", reject);
    });
    req.on("error", reject);
  });
}

function extractTarGz(buf, outDir) {
  fs.mkdirSync(outDir, { recursive: true });
  const tarPath = path.join(outDir, "_download.tar.gz");
  fs.writeFileSync(tarPath, buf);
  try {
    execFileSync("tar", ["-xzf", tarPath, "-C", outDir], { stdio: "pipe" });
  } finally {
    try {
      fs.unlinkSync(tarPath);
    } catch {
      /* ignore */
    }
  }
  // Find the binary (flat or nested).
  const direct = path.join(outDir, "puddly");
  if (fs.existsSync(direct) && fs.statSync(direct).isFile()) {
    return direct;
  }
  const walk = (dir) => {
    for (const name of fs.readdirSync(dir)) {
      const p = path.join(dir, name);
      const st = fs.statSync(p);
      if (st.isDirectory()) {
        const found = walk(p);
        if (found) return found;
      } else if (name === "puddly") {
        return p;
      }
    }
    return null;
  };
  return walk(outDir);
}

async function main() {
  if (process.env.PUDDLY_SKIP_DOWNLOAD === "1") {
    if (fs.existsSync(DEST)) {
      log(`skip download (vendor present): ${DEST}`);
      return;
    }
    warn("PUDDLY_SKIP_DOWNLOAD=1 but vendor/puddly missing — bin shim will fail until you provide a binary");
    return;
  }

  if (process.env.PUDDLY_BINARY) {
    const src = process.env.PUDDLY_BINARY;
    if (!fs.existsSync(src)) {
      throw new Error(`PUDDLY_BINARY not found: ${src}`);
    }
    fs.mkdirSync(VENDOR, { recursive: true });
    fs.copyFileSync(src, DEST);
    fs.chmodSync(DEST, 0o755);
    log(`installed from PUDDLY_BINARY → ${DEST}`);
    return;
  }

  // Already present (e.g. packed with a binary for offline/CI).
  if (fs.existsSync(DEST)) {
    log(`vendor binary already present: ${DEST}`);
    return;
  }

  let platform;
  try {
    platform = detectPlatform();
  } catch (e) {
    warn(String(e.message || e));
    warn("Skipping binary download. Use install.sh or cargo install — see package README.");
    return;
  }

  const version = (process.env.PUDDLY_VERSION || packageVersion()).replace(
    /^v/,
    "",
  );
  const tag = releaseTag(version);
  const repo = process.env.PUDDLY_REPO || DEFAULT_REPO;
  const asset = assetName(version, platform.target);
  const url = releaseDownloadUrl(repo, tag, asset);

  log(`fetching ${asset}…`);
  let buf;
  try {
    buf = await download(url);
  } catch (e) {
    warn(`download failed: ${e.message || e}`);
    warn(`Expected: ${url}`);
    warn(
      "No matching GitHub Release yet. Options:\n" +
        "  • curl -fsSL https://raw.githubusercontent.com/michaelmoses-op/Puddly/main/install.sh | bash\n" +
        "  • cargo install --git https://github.com/michaelmoses-op/Puddly.git --locked puddly-cli\n" +
        "  • For local/CI: PUDDLY_BINARY=/path/to/puddly npm i  (or PUDDLY_SKIP_DOWNLOAD=1 with vendor/)",
    );
    // Soft-fail: do not break `npm i` of a monorepo that only needs the package metadata.
    // The bin shim will print a clear error if invoked without a binary.
    return;
  }

  const tmp = fs.mkdtempSync(path.join(require("os").tmpdir(), "puddly-npm-"));
  try {
    const extracted = extractTarGz(buf, tmp);
    if (!extracted) {
      throw new Error("tarball did not contain a puddly binary");
    }
    fs.mkdirSync(VENDOR, { recursive: true });
    fs.copyFileSync(extracted, DEST);
    fs.chmodSync(DEST, 0o755);
    log(`installed ${DEST}`);
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

main().catch((e) => {
  warn(String(e && e.stack ? e.stack : e));
  warn("postinstall soft-failed; see packages/puddly/README.md");
  // Soft exit so `npm i` in workspaces is not blocked before the first Release exists.
  process.exitCode = 0;
});
