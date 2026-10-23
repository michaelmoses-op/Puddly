#!/usr/bin/env node
"use strict";

/**
 * Thin shim: exec the prebuilt binary installed into vendor/ by postinstall.
 * Does not compile Rust.
 */

const fs = require("fs");
const path = require("path");
const { spawnSync } = require("child_process");

const binary = path.join(__dirname, "..", "vendor", "puddly");

if (!fs.existsSync(binary)) {
  console.error(
    [
      "puddly: native binary missing.",
      "Install a prebuilt binary first:",
      "  curl -fsSL https://raw.githubusercontent.com/michaelmoses-op/Puddly/main/install.sh | bash",
      "Or reinstall this package after a GitHub Release exists for your platform,",
      "or set PUDDLY_BINARY=/path/to/puddly before npm install.",
      `Expected: ${binary}`,
    ].join("\n"),
  );
  process.exit(1);
}

const result = spawnSync(binary, process.argv.slice(2), {
  stdio: "inherit",
  env: process.env,
});

if (result.error) {
  console.error(`puddly: failed to run ${binary}: ${result.error.message}`);
  process.exit(1);
}

process.exit(result.status == null ? 1 : result.status);
