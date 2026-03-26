const fs = require("node:fs");
const path = require("node:path");

const vscode = require("vscode");
const {
  LanguageClient,
  RevealOutputChannelOn,
} = require("vscode-languageclient/node");

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

  const nextClient = createLanguageClient(context);
  client = nextClient;
  context.subscriptions.push(nextClient.start());
  await nextClient.onReady();
}

function createLanguageClient(context) {
  const serverOptions = resolveServerOptions(context);
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

function resolveServerOptions(context) {
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

  const onPath = findOnPath("vest_lsp");
  if (onPath) {
    return {
      command: onPath,
      args: configuredArgs,
      options: { env },
    };
  }

  const message =
    "Could not find a Vest language server binary. Set `vest.languageServer.path` or rebuild the VS Code package.";
  vscode.window.showErrorMessage(message);
  throw new Error(message);
}

function ensureExecutable(executablePath) {
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

  for (const directory of searchPath.split(path.delimiter)) {
    if (!directory) {
      continue;
    }

    const candidate = path.join(directory, binaryName);
    if (fs.existsSync(candidate)) {
      return candidate;
    }
  }

  return undefined;
}

function formatError(error) {
  return error instanceof Error ? error.message : String(error);
}

module.exports = {
  activate,
  deactivate,
};
