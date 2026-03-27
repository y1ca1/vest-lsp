const fs = require("node:fs");
const fsp = require("node:fs/promises");
const https = require("node:https");
const path = require("node:path");
const zlib = require("node:zlib");
const { pipeline } = require("node:stream/promises");

const vscode = require("vscode");
const {
  LanguageClient,
  RevealOutputChannelOn,
} = require("vscode-languageclient/node");

const GITHUB_REPO = "y1ca1/vest-lsp";

let client;
let outputChannel;

function activate(context) {
  outputChannel = vscode.window.createOutputChannel("Vest");
  context.subscriptions.push(outputChannel);

  const restartDisposable = vscode.commands.registerCommand(
    "vest.restartLanguageServer",
    async () => {
      await restartLanguageClient(context);
    },
  );
  context.subscriptions.push(restartDisposable);

  return restartLanguageClient(context);
}

async function deactivate() {
  if (client) {
    const currentClient = client;
    client = undefined;
    await currentClient.stop();
  }
}

async function restartLanguageClient(context) {
  if (client) {
    const currentClient = client;
    client = undefined;
    await currentClient.stop();
  }

  const nextClient = await createLanguageClient(context);
  client = nextClient;
  context.subscriptions.push(nextClient.start());
  await nextClient.onReady();
}

async function createLanguageClient(context) {
  const serverOptions = await resolveServerOptions(context);
  const clientOptions = {
    documentSelector: [
      { scheme: "file", language: "vest" },
      { scheme: "untitled", language: "vest" },
    ],
    outputChannel,
    traceOutputChannel: outputChannel,
    revealOutputChannelOn: RevealOutputChannelOn.Never,
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher("**/*.vest"),
    },
  };

  outputChannel.appendLine(`Starting Vest language server: ${serverOptions.command}`);

  return new LanguageClient(
    "vest-lsp",
    "Vest Language Server",
    serverOptions,
    clientOptions,
  );
}

async function resolveServerOptions(context) {
  const config = vscode.workspace.getConfiguration("vest");
  const configuredPath = config.get("languageServer.path", "").trim();
  const configuredArgs = config.get("languageServer.arguments", []);
  const configuredEnv = config.get("languageServer.environment", {});
  const env = { ...process.env, ...configuredEnv };

  if (configuredPath) {
    ensureExecutable(configuredPath);
    return {
      command: configuredPath,
      args: configuredArgs,
      options: { env },
    };
  }

  const bundledPath = context.asAbsolutePath(path.join("bin", "vest_lsp.bin"));
  if (fs.existsSync(bundledPath)) {
    ensureExecutable(bundledPath);
    return {
      command: bundledPath,
      args: configuredArgs,
      options: { env },
    };
  }

  const downloadedPath = await ensureReleaseBinary(context);
  if (downloadedPath) {
    return {
      command: downloadedPath,
      args: configuredArgs,
      options: { env },
    };
  }

  const onPath = findOnPath(executableName("vest_lsp"));
  if (onPath) {
    return {
      command: onPath,
      args: configuredArgs,
      options: { env },
    };
  }

  const message =
    "Could not find a Vest language server binary. Set `vest.languageServer.path`, install `vest_lsp` on PATH, or use a published extension version with matching GitHub release assets.";
  vscode.window.showErrorMessage(message);
  throw new Error(message);
}

async function ensureReleaseBinary(context) {
  const assetName = releaseAssetName();
  if (!assetName) {
    outputChannel.appendLine(
      `No published Vest language-server asset is available for ${process.platform} ${process.arch}.`,
    );
    return undefined;
  }

  const installRoot = path.join(
    context.globalStorageUri.fsPath,
    "language-server",
    context.extension.packageJSON.version,
  );
  const binaryPath = path.join(installRoot, executableName("vest_lsp"));
  if (fs.existsSync(binaryPath)) {
    ensureExecutable(binaryPath);
    return binaryPath;
  }

  const releaseUrl = releaseAssetUrl(context.extension.packageJSON.version, assetName);
  outputChannel.appendLine(`Downloading Vest language server from ${releaseUrl}`);

  try {
    await vscode.window.withProgress(
      {
        location: vscode.ProgressLocation.Notification,
        title: "Downloading Vest language server",
      },
      async () => {
        await downloadGzipAsset(releaseUrl, binaryPath);
      },
    );
    ensureExecutable(binaryPath);
    return binaryPath;
  } catch (error) {
    outputChannel.appendLine(
      `Failed to download Vest language server from GitHub release: ${formatError(error)}`,
    );
    return undefined;
  }
}

function ensureExecutable(executablePath) {
  if (process.platform === "win32") {
    return;
  }

  try {
    fs.chmodSync(executablePath, 0o755);
  } catch (error) {
    outputChannel.appendLine(
      `Failed to adjust executable permissions for ${executablePath}: ${formatError(error)}`,
    );
  }
}

function findOnPath(binaryName) {
  const searchPath = process.env.PATH;
  if (!searchPath) {
    return undefined;
  }

  const candidateNames =
    process.platform === "win32" && !binaryName.endsWith(".exe")
      ? [binaryName, `${binaryName}.exe`]
      : [binaryName];

  for (const directory of searchPath.split(path.delimiter)) {
    if (!directory) {
      continue;
    }

    for (const candidateName of candidateNames) {
      const candidate = path.join(directory, candidateName);
      if (fs.existsSync(candidate)) {
        return candidate;
      }
    }
  }

  return undefined;
}

function executableName(baseName) {
  return process.platform === "win32" ? `${baseName}.exe` : baseName;
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
    return undefined;
  }

  return `vest_lsp-${osName}-${archName}.gz`;
}

function releaseAssetUrl(version, assetName) {
  return `https://github.com/${GITHUB_REPO}/releases/download/v${version}/${assetName}`;
}

async function downloadGzipAsset(url, destinationPath) {
  const tempPath = `${destinationPath}.download`;
  await fsp.mkdir(path.dirname(destinationPath), { recursive: true });

  try {
    const response = await request(url);
    await pipeline(response, zlib.createGunzip(), fs.createWriteStream(tempPath, { mode: 0o755 }));
    await fsp.rename(tempPath, destinationPath);
  } catch (error) {
    await fsp.rm(tempPath, { force: true }).catch(() => undefined);
    await fsp.rm(destinationPath, { force: true }).catch(() => undefined);
    throw error;
  }
}

function request(url, redirectCount = 0) {
  return new Promise((resolve, reject) => {
    const req = https.get(
      url,
      {
        headers: {
          "user-agent": "vest-vscode-extension",
          accept: "application/octet-stream",
        },
      },
      (response) => {
        const { statusCode = 0, headers } = response;

        if (
          statusCode >= 300 &&
          statusCode < 400 &&
          headers.location &&
          redirectCount < 5
        ) {
          response.resume();
          resolve(request(headers.location, redirectCount + 1));
          return;
        }

        if (statusCode !== 200) {
          response.resume();
          reject(new Error(`unexpected HTTP status ${statusCode}`));
          return;
        }

        resolve(response);
      },
    );

    req.on("error", reject);
  });
}

function formatError(error) {
  return error instanceof Error ? error.message : String(error);
}

module.exports = {
  activate,
  deactivate,
};
