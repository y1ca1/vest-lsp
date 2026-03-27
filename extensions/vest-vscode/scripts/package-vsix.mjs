import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const extensionRoot = path.resolve(scriptDir, "..");
const repoRoot = path.resolve(extensionRoot, "..", "..");
const buildRoot = path.join(extensionRoot, ".build");
const stagedExtensionRoot = path.join(buildRoot, "extension");
const distRoot = path.join(extensionRoot, "dist");
const syncScript = path.join(repoRoot, "scripts", "sync-extension-metadata.mjs");
const VSCE_VERSION = "3.7.1";
const SOURCE_ENTRIES = [
  "extension.js",
  "language-configuration.json",
  "README.md",
  "node_modules",
  "package.json",
  "syntaxes",
];

main();

function main() {
  run(process.execPath, [syncScript], { cwd: repoRoot });

  const packageJson = readJson(path.join(extensionRoot, "package.json"));
  const bundleServer = process.env.VEST_SKIP_BUNDLED_SERVER !== "1";
  const targetPlatform = bundleServer ? detectTargetPlatform() : "universal";

  if (bundleServer) {
    run("cargo", [
      "build",
      "--release",
      "--package",
      "vest_lsp",
      "--bin",
      "vest_lsp",
    ], { cwd: repoRoot });
  }

  fs.rmSync(buildRoot, { recursive: true, force: true });
  fs.mkdirSync(stagedExtensionRoot, { recursive: true });
  fs.mkdirSync(distRoot, { recursive: true });

  copyExtensionSource(stagedExtensionRoot);

  if (bundleServer) {
    const bundledBinarySource = path.join(repoRoot, "target", "release", executableName("vest_lsp"));
    const bundledBinaryTarget = path.join(stagedExtensionRoot, "bin", "vest_lsp.bin");
    fs.mkdirSync(path.dirname(bundledBinaryTarget), { recursive: true });
    fs.copyFileSync(bundledBinarySource, bundledBinaryTarget);
    fs.chmodSync(bundledBinaryTarget, 0o755);
  }

  const licenseSource = path.join(repoRoot, "LICENSE");
  if (fs.existsSync(licenseSource)) {
    fs.copyFileSync(licenseSource, path.join(stagedExtensionRoot, "LICENSE"));
  }

  const vsixBasename = targetPlatform === "universal"
    ? `${packageJson.name}-${packageJson.version}.vsix`
    : `${packageJson.name}-${packageJson.version}-${targetPlatform}.vsix`;
  const vsixPath = path.join(distRoot, vsixBasename);
  fs.rmSync(vsixPath, { force: true });

  run("npx", [
    "--yes",
    `@vscode/vsce@${VSCE_VERSION}`,
    "package",
    "--out",
    vsixPath,
  ], { cwd: stagedExtensionRoot });

  console.log(vsixPath);
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function copyExtensionSource(destinationRoot) {
  for (const entryName of SOURCE_ENTRIES) {
    const sourcePath = path.join(extensionRoot, entryName);
    const destinationPath = path.join(destinationRoot, entryName);
    fs.cpSync(sourcePath, destinationPath, { recursive: true });
  }
}

function detectTargetPlatform() {
  const platforms = {
    darwin: {
      arm64: "darwin-arm64",
      x64: "darwin-x64",
    },
    linux: {
      arm64: "linux-arm64",
      x64: "linux-x64",
    },
    win32: {
      arm64: "win32-arm64",
      x64: "win32-x64",
    },
  };

  const platform = platforms[process.platform]?.[process.arch];
  if (!platform) {
    throw new Error(`Unsupported packaging target: ${process.platform} ${process.arch}`);
  }
  return platform;
}

function executableName(baseName) {
  return process.platform === "win32" ? `${baseName}.exe` : baseName;
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    ...options,
    stdio: "inherit",
  });

  if (result.status !== 0) {
    throw new Error(`Command failed: ${command} ${args.join(" ")}`);
  }
}
