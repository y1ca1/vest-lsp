import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "..");
const args = new Set(process.argv.slice(2));
const checkOnly = args.has("--check");

const workspaceMetadata = readWorkspaceMetadata(path.join(repoRoot, "Cargo.toml"));
const updates = [];

syncJson(path.join(repoRoot, "extensions", "vest-vscode", "package.json"), (json) => {
  json.version = workspaceMetadata.version;
  json.repository = {
    type: "git",
    url: workspaceMetadata.repository,
  };
  json.homepage = workspaceMetadata.repository;
  json.bugs = {
    url: `${workspaceMetadata.repository}/issues`,
  };
  return json;
});

const packageLockPath = path.join(repoRoot, "extensions", "vest-vscode", "package-lock.json");
if (fs.existsSync(packageLockPath)) {
  syncJson(packageLockPath, (json) => {
    json.version = workspaceMetadata.version;
    if (json.packages?.[""]) {
      json.packages[""].version = workspaceMetadata.version;
    }
    return json;
  });
}

syncJson(path.join(repoRoot, "vest_syntax", "package.json"), (json) => {
  json.version = workspaceMetadata.version;
  return json;
});

syncToml(path.join(repoRoot, "extensions", "vest-zed", "Cargo.toml"), [
  { section: "package", key: "version", value: workspaceMetadata.version },
]);

syncToml(path.join(repoRoot, "extensions", "vest-zed", "extension.toml"), [
  { section: null, key: "version", value: workspaceMetadata.version },
  { section: null, key: "repository", value: workspaceMetadata.repository },
  { section: "grammars.vest", key: "repository", value: workspaceMetadata.repository },
]);

syncText(
  path.join(repoRoot, "extensions", "vest-vscode", "extension.js"),
  /const GITHUB_REPO = "[^"]+";/,
  `const GITHUB_REPO = "${workspaceMetadata.repositorySlug}";`,
);

syncText(
  path.join(repoRoot, "extensions", "vest-zed", "src", "lib.rs"),
  /const GITHUB_REPO: &str = "[^"]+";/,
  `const GITHUB_REPO: &str = "${workspaceMetadata.repositorySlug}";`,
);

if (checkOnly && updates.length > 0) {
  for (const filePath of updates) {
    console.error(`metadata out of sync: ${path.relative(repoRoot, filePath)}`);
  }
  process.exit(1);
}

function readWorkspaceMetadata(cargoTomlPath) {
  const content = fs.readFileSync(cargoTomlPath, "utf8");
  const workspaceSection = sectionContent(content, "workspace.package");
  if (!workspaceSection) {
    throw new Error(`missing [workspace.package] in ${cargoTomlPath}`);
  }

  return {
    version: readTomlScalar(workspaceSection, "version"),
    repository: readTomlScalar(workspaceSection, "repository"),
    repositorySlug: repositorySlug(readTomlScalar(workspaceSection, "repository")),
  };
}

function syncJson(filePath, update) {
  const original = fs.readFileSync(filePath, "utf8");
  const updatedValue = update(JSON.parse(original));
  const next = `${JSON.stringify(updatedValue, null, 2)}\n`;
  if (next !== original) {
    updates.push(filePath);
    if (!checkOnly) {
      fs.writeFileSync(filePath, next, "utf8");
    }
  }
}

function syncToml(filePath, changes) {
  const original = fs.readFileSync(filePath, "utf8");
  let next = original;
  for (const change of changes) {
    next = replaceTomlField(next, change.section, change.key, change.value);
  }

  if (next !== original) {
    updates.push(filePath);
    if (!checkOnly) {
      fs.writeFileSync(filePath, next, "utf8");
    }
  }
}

function syncText(filePath, pattern, replacement) {
  const original = fs.readFileSync(filePath, "utf8");
  if (!pattern.test(original)) {
    throw new Error(`could not find pattern in ${filePath}`);
  }

  const next = original.replace(pattern, replacement);
  if (next === original) {
    return;
  }

  updates.push(filePath);
  if (!checkOnly) {
    fs.writeFileSync(filePath, next, "utf8");
  }
}

function sectionContent(content, sectionName) {
  const sections = parseTomlSections(content);
  return sections.get(sectionName ?? "");
}

function readTomlScalar(sectionText, key) {
  const match = sectionText.match(new RegExp(`^${escapeRegExp(key)}\\s*=\\s*"([^"]+)"$`, "m"));
  if (!match) {
    throw new Error(`missing \`${key}\``);
  }
  return match[1];
}

function replaceTomlField(content, sectionName, key, value) {
  const lines = content.split("\n");
  let currentSection = "";
  let replaced = false;

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    const sectionMatch = line.match(/^\[(.+)\]$/);
    if (sectionMatch) {
      currentSection = sectionMatch[1];
      continue;
    }

    if (currentSection !== (sectionName ?? "")) {
      continue;
    }

    if (new RegExp(`^${escapeRegExp(key)}\\s*=\\s*`).test(line)) {
      lines[index] = `${key} = "${value}"`;
      replaced = true;
      break;
    }
  }

  if (!replaced) {
    const target = sectionName ? `[${sectionName}]` : "top-level section";
    throw new Error(`could not find \`${key}\` in ${target}`);
  }

  return lines.join("\n");
}

function parseTomlSections(content) {
  const sections = new Map();
  let currentSection = "";
  let currentLines = [];

  for (const line of content.split("\n")) {
    const sectionMatch = line.match(/^\[(.+)\]$/);
    if (sectionMatch) {
      sections.set(currentSection, currentLines.join("\n"));
      currentSection = sectionMatch[1];
      currentLines = [];
      continue;
    }

    currentLines.push(line);
  }

  sections.set(currentSection, currentLines.join("\n"));
  return sections;
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function repositorySlug(repositoryUrl) {
  const url = new URL(repositoryUrl);
  return url.pathname.replace(/^\/+/, "").replace(/\.git$/, "");
}
