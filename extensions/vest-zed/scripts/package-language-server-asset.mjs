import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const extensionRoot = path.resolve(scriptDir, "..");
const repoRoot = path.resolve(extensionRoot, "..", "..");
const distRoot = path.join(extensionRoot, "dist");

main();

function main() {
  run(process.execPath, [path.join(repoRoot, "scripts", "sync-extension-metadata.mjs")], {
    cwd: repoRoot,
  });

  run("cargo", [
    "build",
    "--release",
    "--package",
    "vest_lsp",
    "--bin",
    "vest_lsp",
  ], { cwd: repoRoot });

  fs.mkdirSync(distRoot, { recursive: true });

  const assetName = releaseAssetName();
  const binaryName = process.platform === "win32" ? "vest_lsp.exe" : "vest_lsp";
  const binaryPath = path.join(repoRoot, "target", "release", binaryName);
  const assetPath = path.join(distRoot, assetName);

  const compressed = zlib.gzipSync(fs.readFileSync(binaryPath));
  fs.writeFileSync(assetPath, compressed);

  console.log(assetPath);
}

function releaseAssetName() {
  const osName = {
    darwin: "mac",
    linux: "linux",
    win32: "windows",
  }[process.platform];
  const archName = {
    arm64: "aarch64",
    x64: "x8664",
  }[process.arch];

  if (!osName || !archName) {
    throw new Error(`Unsupported release asset platform: ${process.platform} ${process.arch}`);
  }

  return `vest_lsp-${osName}-${archName}.gz`;
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { ...options, stdio: "inherit" });
  if (result.status !== 0) {
    throw new Error(`Command failed: ${command} ${args.join(" ")}`);
  }
}
