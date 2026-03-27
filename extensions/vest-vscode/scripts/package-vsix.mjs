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
  const hasLicense = fs.existsSync(path.join(repoRoot, "LICENSE"));

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

  fs.writeFileSync(
    path.join(buildRoot, "[Content_Types].xml"),
    renderContentTypes(),
    "utf8",
  );
  fs.writeFileSync(
    path.join(buildRoot, "extension.vsixmanifest"),
    renderVsixManifest(packageJson, targetPlatform, hasLicense),
    "utf8",
  );

  const vsixBasename = targetPlatform === "universal"
    ? `${packageJson.name}-${packageJson.version}.vsix`
    : `${packageJson.name}-${packageJson.version}-${targetPlatform}.vsix`;
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
  for (const entryName of SOURCE_ENTRIES) {
    const sourcePath = path.join(extensionRoot, entryName);
    const destinationPath = path.join(destinationRoot, entryName);
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

function renderVsixManifest(packageJson, targetPlatform, hasLicense) {
  const properties = [
    propertyXml("Microsoft.VisualStudio.Code.Engine", packageJson.engines.vscode),
    propertyXml("Microsoft.VisualStudio.Code.ExtensionDependencies", ""),
    propertyXml("Microsoft.VisualStudio.Code.ExtensionPack", ""),
    propertyXml("Microsoft.VisualStudio.Code.ExtensionKind", "workspace"),
    propertyXml("Microsoft.VisualStudio.Code.LocalizedLanguages", ""),
    propertyXml("Microsoft.VisualStudio.Code.EnabledApiProposals", ""),
    propertyXml("Microsoft.VisualStudio.Code.ExecutesCode", "true"),
  ];

  if (targetPlatform !== "universal") {
    properties.splice(1, 0, propertyXml("Microsoft.VisualStudio.Code.TargetPlatform", targetPlatform));
  }

  const sourceUrl = packageJson.repository?.url;
  if (sourceUrl) {
    properties.push(propertyXml("Microsoft.VisualStudio.Services.Links.Source", sourceUrl));
    properties.push(propertyXml("Microsoft.VisualStudio.Services.Links.Getstarted", sourceUrl));
    properties.push(propertyXml("Microsoft.VisualStudio.Services.Links.GitHub", sourceUrl));
  }

  if (packageJson.bugs?.url) {
    properties.push(propertyXml("Microsoft.VisualStudio.Services.Links.Support", packageJson.bugs.url));
  }

  if (packageJson.homepage) {
    properties.push(propertyXml("Microsoft.VisualStudio.Services.Links.Learn", packageJson.homepage));
  }

  properties.push(propertyXml("Microsoft.VisualStudio.Services.GitHubFlavoredMarkdown", "true"));
  properties.push(propertyXml("Microsoft.VisualStudio.Services.Content.Pricing", "Free"));

  const assets = [
    '<Asset Type="Microsoft.VisualStudio.Code.Manifest" Path="extension/package.json" Addressable="true" />',
    '<Asset Type="Microsoft.VisualStudio.Services.Content.Details" Path="extension/README.md" Addressable="true" />',
  ];

  if (hasLicense) {
    assets.push('<Asset Type="Microsoft.VisualStudio.Services.Content.License" Path="extension/LICENSE" Addressable="true" />');
  }

  return `<?xml version="1.0" encoding="utf-8"?>
<PackageManifest Version="2.0.0" xmlns="http://schemas.microsoft.com/developer/vsx-schema/2011" xmlns:d="http://schemas.microsoft.com/developer/vsx-schema-design/2011">
  <Metadata>
    <Identity
      Language="en-US"
      Id="${xmlEscape(packageJson.name)}"
      Version="${xmlEscape(packageJson.version)}"
      Publisher="${xmlEscape(packageJson.publisher)}" />
    <DisplayName>${xmlEscape(packageJson.displayName)}</DisplayName>
    <Description xml:space="preserve">${xmlEscape(packageJson.description)}</Description>
    <Tags>${xmlEscape((packageJson.keywords || []).join(","))}</Tags>
    <Categories>${xmlEscape((packageJson.categories || []).join(","))}</Categories>
    <GalleryFlags>Public</GalleryFlags>
    <Properties>
${properties.map((property) => `      ${property}`).join("\n")}
    </Properties>
  </Metadata>
  <Installation>
    <InstallationTarget Id="Microsoft.VisualStudio.Code" />
  </Installation>
  <Dependencies />
  <Assets>
${assets.map((asset) => `    ${asset}`).join("\n")}
  </Assets>
</PackageManifest>
`;
}

function propertyXml(id, value) {
  return `<Property Id="${xmlEscape(id)}" Value="${xmlEscape(value)}" />`;
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
