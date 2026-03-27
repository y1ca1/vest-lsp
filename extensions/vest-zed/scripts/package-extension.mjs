import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const extensionRoot = path.resolve(scriptDir, "..");
const repoRoot = path.resolve(extensionRoot, "..", "..");
const distRoot = path.join(extensionRoot, "dist");
const stagedRoot = path.join(distRoot, "vest-zed");

main();

function main() {
  run(process.execPath, [path.join(repoRoot, "scripts", "sync-extension-metadata.mjs")], {
    cwd: repoRoot,
  });

  run("cargo", ["build", "--release", "--target", "wasm32-wasip2"], {
    cwd: extensionRoot,
  });

  const builtWasm = path.join(
    extensionRoot,
    "target",
    "wasm32-wasip2",
    "release",
    "vest_zed_extension.wasm",
  );
  const checkedInWasm = path.join(extensionRoot, "extension.wasm");

  fs.copyFileSync(builtWasm, checkedInWasm);
  fs.rmSync(stagedRoot, { recursive: true, force: true });
  fs.mkdirSync(stagedRoot, { recursive: true });

  copy(path.join(extensionRoot, "extension.toml"), path.join(stagedRoot, "extension.toml"));
  copy(path.join(extensionRoot, "extension.wasm"), path.join(stagedRoot, "extension.wasm"));
  copy(path.join(extensionRoot, "README.md"), path.join(stagedRoot, "README.md"));
  copyDirectory(path.join(extensionRoot, "languages"), path.join(stagedRoot, "languages"));
  copy(
    path.join(extensionRoot, "grammars", "vest.wasm"),
    path.join(stagedRoot, "grammars", "vest.wasm"),
  );

  console.log(stagedRoot);
}

function copy(source, destination) {
  fs.mkdirSync(path.dirname(destination), { recursive: true });
  fs.copyFileSync(source, destination);
}

function copyDirectory(source, destination) {
  fs.cpSync(source, destination, { recursive: true });
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { ...options, stdio: "inherit" });
  if (result.status !== 0) {
    throw new Error(`Command failed: ${command} ${args.join(" ")}`);
  }
}
