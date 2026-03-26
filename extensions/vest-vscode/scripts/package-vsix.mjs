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

main();

function main() {
  const packageJson = readJson(path.join(extensionRoot, "package.json"));
  const targetPlatform = detectTargetPlatform();
  const minimumVscodeVersion = packageJson.engines.vscode.replace(/^\^/, "");

  run("cargo", [
    "build",
    "--release",
    "--package",
    "vest_lsp",
    "--bin",
    "vest_lsp",
  ], { cwd: repoRoot });

  fs.rmSync(buildRoot, { recursive: true, force: true });
  fs.mkdirSync(stagedExtensionRoot, { recursive: true });
  fs.mkdirSync(distRoot, { recursive: true });

  copyExtensionSource(stagedExtensionRoot);

  const bundledBinarySource = path.join(repoRoot, "target", "release", executableName("vest_lsp"));
  const bundledBinaryTarget = path.join(stagedExtensionRoot, "bin", "vest_lsp.bin");
  fs.mkdirSync(path.dirname(bundledBinaryTarget), { recursive: true });
  fs.copyFileSync(bundledBinarySource, bundledBinaryTarget);
  fs.chmodSync(bundledBinaryTarget, 0o755);

  const licenseSource = path.join(repoRoot, "LICENSE");
  if (fs.existsSync(licenseSource)) {
    fs.copyFileSync(licenseSource, path.join(stagedExtensionRoot, "LICENSE"));
  }

  fs.writeFileSync(
    path.join(buildRoot, "[Content_Types].xml"),
    renderContentTypes(),
    "utf8",
  );
  fs.writeFileSync(
    path.join(buildRoot, "extension.vsixmanifest"),
    renderVsixManifest(packageJson, minimumVscodeVersion, targetPlatform),
    "utf8",
  );

  const vsixBasename = `${packageJson.name}-${packageJson.version}-${targetPlatform}.vsix`;
  const vsixPath = path.join(distRoot, vsixBasename);
  fs.rmSync(vsixPath, { force: true });

  run(
    "zip",
    ["-qr", vsixPath, "[Content_Types].xml", "extension.vsixmanifest", "extension"],
    { cwd: buildRoot },
  );

  console.log(vsixPath);
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function copyExtensionSource(destinationRoot) {
  for (const entry of fs.readdirSync(extensionRoot, { withFileTypes: true })) {
    if (entry.name === ".build" || entry.name === "dist" || entry.name === "scripts") {
      continue;
    }

    const sourcePath = path.join(extensionRoot, entry.name);
    const destinationPath = path.join(destinationRoot, entry.name);
    fs.cpSync(sourcePath, destinationPath, { recursive: true });
  }
}

function renderContentTypes() {
  return `<?xml version="1.0" encoding="utf-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="bin" ContentType="application/octet-stream" />
  <Default Extension="json" ContentType="application/json" />
  <Default Extension="js" ContentType="application/javascript" />
  <Default Extension="md" ContentType="text/markdown" />
  <Default Extension="txt" ContentType="text/plain" />
  <Default Extension="vsixmanifest" ContentType="text/xml" />
  <Default Extension="xml" ContentType="text/xml" />
  <Override PartName="/extension.vsixmanifest" ContentType="text/xml" />
</Types>
`;
}

function renderVsixManifest(packageJson, minimumVscodeVersion, targetPlatform) {
  const extensionId = `${packageJson.publisher}.${packageJson.name}`;

  return `<?xml version="1.0" encoding="utf-8"?>
<PackageManifest Version="2.0.0" xmlns="http://schemas.microsoft.com/developer/vsx-schema/2011">
  <Metadata>
    <Identity
      Language="en-US"
      Id="${xmlEscape(extensionId)}"
      Version="${xmlEscape(packageJson.version)}"
      Publisher="${xmlEscape(packageJson.publisher)}" />
    <DisplayName>${xmlEscape(packageJson.displayName)}</DisplayName>
    <Description xml:space="preserve">${xmlEscape(packageJson.description)}</Description>
    <Tags>${xmlEscape((packageJson.keywords || []).join(","))}</Tags>
    <Categories>${xmlEscape((packageJson.categories || []).join(","))}</Categories>
    <Properties>
      <Property Id="Microsoft.VisualStudio.Code.Engine" Value="${xmlEscape(packageJson.engines.vscode)}" />
      <Property Id="Microsoft.VisualStudio.Code.TargetPlatform" Value="${xmlEscape(targetPlatform)}" />
      <Property Id="Microsoft.VisualStudio.Code.ExtensionKind" Value="workspace" />
    </Properties>
  </Metadata>
  <Installation>
    <InstallationTarget Id="Microsoft.VisualStudio.Code" Version="${xmlEscape(minimumVscodeVersion)}" />
  </Installation>
  <Dependencies />
  <Assets>
    <Asset Type="Microsoft.VisualStudio.Code.Manifest" Path="extension/package.json" />
    <Asset Type="Microsoft.VisualStudio.Services.Content.Details" Path="extension/README.md" />
    <Asset Type="Microsoft.VisualStudio.Services.Content.License" Path="extension/LICENSE" />
  </Assets>
</PackageManifest>
`;
}

function xmlEscape(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll("\"", "&quot;")
    .replaceAll("'", "&apos;");
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
