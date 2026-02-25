const path = require("path");
const fs = require("fs");
const vscode = require("vscode");
const {
    LanguageClient,
    RevealOutputChannelOn,
    TransportKind,
    Trace,
} = require("vscode-languageclient/node");

let languageClient;
let outputChannel;
let traceChannel;

function activate(context) {
    outputChannel = vscode.window.createOutputChannel("Coppice LSP Dev");
    traceChannel = vscode.window.createOutputChannel("Coppice LSP Trace");
    const workspaceFolder = vscode.workspace.workspaceFolders &&
        vscode.workspace.workspaceFolders[0];
    if (!workspaceFolder) {
        outputChannel.appendLine(
            "[activate] no workspace folder; refusing to start language client",
        );
        vscode.window.showErrorMessage(
            "Coppice LSP (Dev): open a workspace folder before starting the language server.",
        );
        return;
    }

    const workspaceRoot = workspaceFolder.uri.fsPath;
    const configuredWorkspaceRoot = vscode.workspace
        .getConfiguration("coppice")
        .get("workspaceRoot");
    // TODO: if we were to publish the extension, we would need to make server
    // command configurable via extension settings.
    const serverCommand = path.join(workspaceRoot, "bin", "coppice");
    const serverArgs = configuredWorkspaceRoot
        ? ["--workspace-root", configuredWorkspaceRoot, "lsp"]
        : ["lsp"];
    outputChannel.appendLine(`[activate] workspaceFolder=${workspaceRoot}`);
    outputChannel.appendLine(`[activate] serverCommand=${serverCommand}`);
    outputChannel.appendLine(
        `[activate] serverArgs=${JSON.stringify(serverArgs)}`,
    );
    outputChannel.appendLine(
        `[activate] commandExists=${fs.existsSync(serverCommand)} executable=${
            fs.existsSync(serverCommand)
                ? Boolean(fs.statSync(serverCommand).mode & 0o111)
                : false
        }`,
    );

    const serverOptions = {
        command: serverCommand,
        args: serverArgs,
        transport: TransportKind.stdio,
        options: {
            cwd: workspaceRoot,
        },
    };

    const clientOptions = {
        documentSelector: [{ scheme: "file", language: "coppice" }],
        synchronize: {
            fileEvents: vscode.workspace.createFileSystemWatcher("**/*.copp"),
        },
        outputChannel,
        traceOutputChannel: traceChannel,
        revealOutputChannelOn: RevealOutputChannelOn.Error,
    };

    languageClient = new LanguageClient(
        "coppice-lsp-dev",
        "Coppice LSP (Dev)",
        serverOptions,
        clientOptions,
    );
    languageClient.setTrace(Trace.Verbose);

    languageClient.onDidChangeState((event) => {
        outputChannel.appendLine(
            `[state] old=${event.oldState} new=${event.newState}`,
        );
    });

    const disposable = languageClient.start();
    context.subscriptions.push(disposable);

    context.subscriptions.push(outputChannel, traceChannel);
}

function deactivate() {
    if (!languageClient) {
        return undefined;
    }
    return languageClient.stop();
}

module.exports = {
    activate,
    deactivate,
};
