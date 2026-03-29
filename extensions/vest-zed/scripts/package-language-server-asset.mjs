import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const extensionRoot = path.resolve(scriptDir, "..");
const repoRoot = path.resolve(extensionRoot, "..", "..");
const distRoot = path.join(extensionRoot, "dist");
const targetTriple = process.env.VEST_TARGET_TRIPLE;

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
    ...(targetTriple ? ["--target", targetTriple] : []),
  ], { cwd: repoRoot });

  fs.mkdirSync(distRoot, { recursive: true });

  const metadata = releaseTargetMetadata();
  const assetName = metadata.assetName;
  const binaryPath = path.join(
    repoRoot,
    "target",
    ...(targetTriple ? [targetTriple] : []),
    "release",
    metadata.binaryName,
  );
  const assetPath = path.join(distRoot, assetName);

  const compressed = zlib.gzipSync(fs.readFileSync(binaryPath));
  fs.writeFileSync(assetPath, compressed);

  console.log(assetPath);
}

function releaseTargetMetadata() {
  if (targetTriple) {
    const metadata = supportedTargets().get(targetTriple);
    if (!metadata) {
      throw new Error(`Unsupported release target: ${targetTriple}`);
    }
    return metadata;
  }

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

  return {
    assetName: `vest_lsp-${osName}-${archName}.gz`,
    binaryName: process.platform === "win32" ? "vest_lsp.exe" : "vest_lsp",
  };
}

function supportedTargets() {
  return new Map([
    [
      "x86_64-unknown-linux-gnu",
      { assetName: "vest_lsp-linux-x8664.gz", binaryName: "vest_lsp" },
    ],
    [
      "aarch64-unknown-linux-gnu",
      { assetName: "vest_lsp-linux-aarch64.gz", binaryName: "vest_lsp" },
    ],
    [
      "x86_64-apple-darwin",
      { assetName: "vest_lsp-mac-x8664.gz", binaryName: "vest_lsp" },
    ],
    [
      "aarch64-apple-darwin",
      { assetName: "vest_lsp-mac-aarch64.gz", binaryName: "vest_lsp" },
    ],
    [
      "x86_64-pc-windows-msvc",
      { assetName: "vest_lsp-windows-x8664.gz", binaryName: "vest_lsp.exe" },
    ],
    [
      "aarch64-pc-windows-msvc",
      { assetName: "vest_lsp-windows-aarch64.gz", binaryName: "vest_lsp.exe" },
    ],
  ]);
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { ...options, stdio: "inherit" });
  if (result.status !== 0) {
    throw new Error(`Command failed: ${command} ${args.join(" ")}`);
  }
}
