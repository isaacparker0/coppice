function defaultProgram() {
    return `function main() -> nil {
    print("hello, world")
    return nil
}`;
}

let monacoApi = null;
let editorInstance = null;
let onChangeCallback = null;

function loadScriptOnce(src) {
    return new Promise((resolve, reject) => {
        const existing = document.querySelector(`script[data-src="${src}"]`);
        if (existing) {
            if (existing.dataset.loaded === "true") {
                resolve();
            } else {
                existing.addEventListener("load", () => resolve(), {
                    once: true,
                });
                existing.addEventListener(
                    "error",
                    () => reject(new Error(`failed to load script: ${src}`)),
                    { once: true },
                );
            }
            return;
        }

        const script = document.createElement("script");
        script.src = src;
        script.async = true;
        script.dataset.src = src;
        script.addEventListener(
            "load",
            () => {
                script.dataset.loaded = "true";
                resolve();
            },
            { once: true },
        );
        script.addEventListener(
            "error",
            () => reject(new Error(`failed to load script: ${src}`)),
            { once: true },
        );
        document.head.appendChild(script);
    });
}

async function loadMonaco() {
    if (window.monaco && window.monaco.editor) {
        return window.monaco;
    }

    await loadScriptOnce("/vs/loader.js");
    if (!window.require) {
        throw new Error("monaco loader did not expose window.require");
    }

    window.require.config({ paths: { vs: "/vs" } });
    return new Promise((resolve, reject) => {
        window.require(
            ["vs/editor/editor.main"],
            () => resolve(window.monaco),
            (error) =>
                reject(
                    new Error(
                        `failed to initialize monaco editor: ${String(error)}`,
                    ),
                ),
        );
    });
}

async function initEditor(options) {
    const mount = options && options.mount ? options.mount : null;
    if (!mount) {
        throw new Error("missing editor mount element");
    }

    if (!monacoApi) {
        monacoApi = await loadMonaco();
    }
    if (editorInstance) {
        return;
    }

    onChangeCallback = options && options.onChange ? options.onChange : null;
    editorInstance = monacoApi.editor.create(mount, {
        value: defaultProgram(),
        language: "plaintext",
        automaticLayout: true,
        minimap: { enabled: false },
        fontFamily: "JetBrains Mono, SFMono-Regular, monospace",
        fontSize: 14,
        scrollBeyondLastLine: false,
        fixedOverflowWidgets: true,
    });

    editorInstance.onDidChangeModelContent(() => {
        if (onChangeCallback) {
            onChangeCallback();
        }
    });
}

function getSource() {
    if (!editorInstance) {
        return "";
    }
    return editorInstance.getValue();
}

function setSource(source) {
    if (!editorInstance) {
        return;
    }
    editorInstance.setValue(source);
}

function applyInlineMarkers(diagnostics) {
    if (!editorInstance || !monacoApi) {
        return;
    }
    const model = editorInstance.getModel();
    if (!model) {
        return;
    }

    const markers = [];
    for (const diagnostic of diagnostics || []) {
        if (diagnostic.path && !diagnostic.path.endsWith("main.bin.coppice")) {
            continue;
        }

        const startOffset = Math.max(0, diagnostic.span.start);
        const endOffset = Math.max(startOffset + 1, diagnostic.span.end);
        const startPosition = model.getPositionAt(startOffset);
        const endPosition = model.getPositionAt(endOffset);
        markers.push({
            startLineNumber: startPosition.lineNumber,
            startColumn: startPosition.column,
            endLineNumber: endPosition.lineNumber,
            endColumn: endPosition.column,
            message: diagnostic.message,
            severity: monacoApi.MarkerSeverity.Error,
        });
    }

    monacoApi.editor.setModelMarkers(model, "playground-check", markers);
}

function renderDiagnostics(diagnostics) {
    const diagnosticsElement = document.getElementById("diagnostics");
    diagnosticsElement.innerHTML = "";
    applyInlineMarkers(diagnostics);

    if (!diagnostics || diagnostics.length === 0) {
        const row = document.createElement("li");
        row.textContent = "no diagnostics";
        diagnosticsElement.appendChild(row);
        return;
    }

    for (const diagnostic of diagnostics) {
        const row = document.createElement("li");
        row.className = "error";
        row.textContent =
            `${diagnostic.path}:${diagnostic.span.line}:${diagnostic.span.column} ${diagnostic.message} (${diagnostic.phase})`;

        diagnosticsElement.appendChild(row);
    }
}

function setStatus(text) {
    document.getElementById("status").textContent = text;
}

window.playgroundEditor = {
    initEditor,
    defaultProgram,
    getSource,
    setSource,
    renderDiagnostics,
    setStatus,
};
