"use strict";

/**
 * Map process.platform / process.arch → Rust target triple used by Releases.
 * @returns {{ target: string, binaryName: string }}
 */
function detectPlatform() {
  const { platform, arch } = process;
  let os;
  let cpu;
  switch (platform) {
    case "linux":
      os = "linux";
      break;
    case "darwin":
      os = "darwin";
      break;
    default:
      throw new Error(
        `Unsupported platform: ${platform} (Puddly prebuilt binaries are Linux/macOS only for now)`,
      );
  }
  switch (arch) {
    case "x64":
      cpu = "x86_64";
      break;
    case "arm64":
      cpu = "aarch64";
      break;
    default:
      throw new Error(`Unsupported architecture: ${arch}`);
  }
  const target =
    os === "linux"
      ? `${cpu}-unknown-linux-gnu`
      : `${cpu}-apple-darwin`;
  return { target, binaryName: "puddly" };
}

function packageVersion() {
  // Prefer npm_package_version during install; else read package.json.
  if (process.env.npm_package_version) {
    return process.env.npm_package_version.replace(/^v/, "");
  }
  // eslint-disable-next-line n/no-missing-require
  return require("../package.json").version.replace(/^v/, "");
}

function releaseTag(version) {
  const v = version.replace(/^v/, "");
  return `v${v}`;
}

function assetName(version, target) {
  return `puddly-${version.replace(/^v/, "")}-${target}.tar.gz`;
}

function releaseDownloadUrl(repo, tag, asset) {
  return `https://github.com/${repo}/releases/download/${tag}/${asset}`;
}

module.exports = {
  detectPlatform,
  packageVersion,
  releaseTag,
  assetName,
  releaseDownloadUrl,
  DEFAULT_REPO: "michaelmoses-op/Puddly",
};
