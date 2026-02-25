import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import * as monaco from "monaco-editor";

import {
    checkWorkspace,
    createSession,
    type Diagnostic,
    runWorkspace,
    type WorkspaceFile,
    type WorkspaceRequest,
} from "./api";

const CHECK_DEBOUNCE_MS = 220;
const MARKER_OWNER = "playground-check";

type WorkspaceState = {
    filesByPath: Map<string, string>;
    fileOrder: string[];
    activeFilePath: string | null;
    entrypointPath: string | null;
};

function defaultWorkspaceFiles(): WorkspaceFile[] {
    return [
        { path: "PACKAGE.copp", source: "" },
        {
            path: "main.bin.copp",
            source: `function main() -> nil {
    print("hello, world")
    return nil
}`,
        },
    ];
}

function createModelUri(path: string): monaco.Uri {
    return monaco.Uri.parse(`inmemory://workspace/${encodeURI(path)}`);
}

function initializeWorkspaceState(
    files: WorkspaceFile[],
    preferredEntrypointPath: string | null,
    preferredActivePath: string | null,
): WorkspaceState {
    const filesByPath = new Map<string, string>();
    const fileOrder: string[] = [];

    for (const file of files) {
        filesByPath.set(file.path, file.source);
        fileOrder.push(file.path);
    }

    if (!filesByPath.has("PACKAGE.copp")) {
        filesByPath.set("PACKAGE.copp", "");
        fileOrder.unshift("PACKAGE.copp");
    }

    const binPaths = fileOrder.filter((path) => path.endsWith(".bin.copp"));
    const entrypointPath =
        preferredEntrypointPath && filesByPath.has(preferredEntrypointPath)
            ? preferredEntrypointPath
            : (binPaths[0] ?? null);
    const activeFilePath =
        preferredActivePath && filesByPath.has(preferredActivePath)
            ? preferredActivePath
            : (fileOrder[0] ?? null);

    return { filesByPath, fileOrder, activeFilePath, entrypointPath };
}

function workspaceRequestPayload(state: WorkspaceState): WorkspaceRequest {
    return {
        entrypoint_path: state.entrypointPath,
        files: state.fileOrder.map((path) => ({
            path,
            source: state.filesByPath.get(path) ?? "",
        })),
    };
}

export function App() {
    const initialWorkspace = useMemo(
        () =>
            initializeWorkspaceState(
                defaultWorkspaceFiles(),
                "main.bin.copp",
                "main.bin.copp",
            ),
        [],
    );

    const [workspace, setWorkspace] = useState<WorkspaceState>(
        initialWorkspace,
    );
    const [diagnostics, setDiagnostics] = useState<Diagnostic[]>([]);
    const [stdout, setStdout] = useState("");
    const [stderr, setStderr] = useState("");
    const [statusText, setStatusText] = useState("idle");
    const [isRunDisabled, setIsRunDisabled] = useState(false);

    const workspaceRef = useRef(workspace);
    const sessionIdRef = useRef<string | null>(null);
    const activeCheckControllerRef = useRef<AbortController | null>(null);
    const checkTimerRef = useRef<number | null>(null);
    const isMountedRef = useRef(true);

    const editorContainerRef = useRef<HTMLDivElement | null>(null);
    const editorRef = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
    const modelByPathRef = useRef(new Map<string, monaco.editor.ITextModel>());
    const pathByModelUriRef = useRef(new Map<string, string>());

    useEffect(() => {
        workspaceRef.current = workspace;
    }, [workspace]);

    const updateWorkspace = useCallback(
        (updater: (previousWorkspace: WorkspaceState) => WorkspaceState) => {
            setWorkspace((previousWorkspace) => {
                const nextWorkspace = updater(previousWorkspace);
                workspaceRef.current = nextWorkspace;
                return nextWorkspace;
            });
        },
        [],
    );

    const ensureSession = useCallback(async (): Promise<string> => {
        if (sessionIdRef.current) {
            return sessionIdRef.current;
        }
        const session = await createSession();
        sessionIdRef.current = session.session_id;
        return session.session_id;
    }, []);

    const applyDiagnosticMarkers = useCallback(
        (state: WorkspaceState, nextDiagnostics: Diagnostic[]) => {
            for (const model of modelByPathRef.current.values()) {
                monaco.editor.setModelMarkers(model, MARKER_OWNER, []);
            }

            const diagnosticsByPath = new Map<string, Diagnostic[]>();
            for (const diagnostic of nextDiagnostics) {
                if (!state.filesByPath.has(diagnostic.path)) {
                    continue;
                }
                const entries = diagnosticsByPath.get(diagnostic.path) ?? [];
                entries.push(diagnostic);
                diagnosticsByPath.set(diagnostic.path, entries);
            }

            for (const [path, model] of modelByPathRef.current.entries()) {
                const pathDiagnostics = diagnosticsByPath.get(path) ?? [];
                const markers: monaco.editor.IMarkerData[] = pathDiagnostics
                    .map(
                        (diagnostic) => {
                            const startOffset = Math.max(
                                0,
                                diagnostic.span.start,
                            );
                            const endOffset = Math.max(
                                startOffset + 1,
                                diagnostic.span.end,
                            );
                            const startPosition = model.getPositionAt(
                                startOffset,
                            );
                            const endPosition = model.getPositionAt(endOffset);
                            return {
                                startLineNumber: startPosition.lineNumber,
                                startColumn: startPosition.column,
                                endLineNumber: endPosition.lineNumber,
                                endColumn: endPosition.column,
                                message: diagnostic.message,
                                severity: monaco.MarkerSeverity.Error,
                            };
                        },
                    );
                monaco.editor.setModelMarkers(model, MARKER_OWNER, markers);
            }
        },
        [],
    );

    const runCheck = useCallback(async () => {
        const state = workspaceRef.current;
        const sessionId = await ensureSession();

        if (!state.entrypointPath) {
            setDiagnostics([]);
            applyDiagnosticMarkers(state, []);
            setStatusText("add a .bin.copp entrypoint");
            return;
        }

        if (activeCheckControllerRef.current) {
            activeCheckControllerRef.current.abort();
        }
        const checkController = new AbortController();
        activeCheckControllerRef.current = checkController;

        setStatusText("checking...");
        const result = await checkWorkspace(
            sessionId,
            workspaceRequestPayload(state),
            checkController.signal,
        ).catch((error: unknown) => {
            if (error instanceof Error && error.name === "AbortError") {
                return null;
            }
            throw error;
        });

        if (!result || !isMountedRef.current) {
            return;
        }

        const nextDiagnostics = result.data.diagnostics ?? [];
        setDiagnostics(nextDiagnostics);
        applyDiagnosticMarkers(state, nextDiagnostics);
        if (result.data.error) {
            setStatusText(`check failed: ${result.data.error.message}`);
            return;
        }
        setStatusText(result.data.ok ? "ok" : "diagnostics");
    }, [applyDiagnosticMarkers, ensureSession]);

    const scheduleCheck = useCallback(() => {
        if (checkTimerRef.current !== null) {
            window.clearTimeout(checkTimerRef.current);
        }
        checkTimerRef.current = window.setTimeout(() => {
            void runCheck();
        }, CHECK_DEBOUNCE_MS);
    }, [runCheck]);

    const runProgram = useCallback(async () => {
        const state = workspaceRef.current;
        const sessionId = await ensureSession();

        if (!state.entrypointPath) {
            setStatusText("add a .bin.copp entrypoint");
            return;
        }

        setStatusText("running...");
        setIsRunDisabled(true);
        try {
            const result = await runWorkspace(
                sessionId,
                workspaceRequestPayload(state),
            );
            const nextDiagnostics = result.data.diagnostics ?? [];
            setDiagnostics(nextDiagnostics);
            applyDiagnosticMarkers(state, nextDiagnostics);

            setStdout(result.data.stdout ?? "");
            setStderr(result.data.stderr ?? "");

            if (result.data.error) {
                setStatusText(`run error: ${result.data.error.message}`);
                return;
            }
            if (result.data.timed_out) {
                setStatusText("run timed out");
                return;
            }
            setStatusText(`exit code ${result.data.exit_code ?? 0}`);
        } finally {
            if (isMountedRef.current) {
                setIsRunDisabled(false);
            }
        }
    }, [applyDiagnosticMarkers, ensureSession]);

    const syncEditorWorkspace = useCallback((state: WorkspaceState) => {
        const editor = editorRef.current;
        if (!editor) {
            return;
        }

        const nextPathSet = new Set(state.fileOrder);
        for (const [existingPath, model] of modelByPathRef.current.entries()) {
            if (nextPathSet.has(existingPath)) {
                continue;
            }
            pathByModelUriRef.current.delete(model.uri.toString());
            model.dispose();
            modelByPathRef.current.delete(existingPath);
        }

        for (const path of state.fileOrder) {
            const source = state.filesByPath.get(path) ?? "";
            const existingModel = modelByPathRef.current.get(path);
            if (!existingModel) {
                const model = monaco.editor.createModel(
                    source,
                    "plaintext",
                    createModelUri(path),
                );
                modelByPathRef.current.set(path, model);
                pathByModelUriRef.current.set(model.uri.toString(), path);
                continue;
            }
            if (existingModel.getValue() !== source) {
                existingModel.setValue(source);
            }
        }

        if (!state.activeFilePath) {
            editor.setModel(null);
            return;
        }
        const activeModel = modelByPathRef.current.get(state.activeFilePath);
        if (activeModel && editor.getModel() !== activeModel) {
            editor.setModel(activeModel);
        }
    }, []);

    useEffect(() => {
        const mount = editorContainerRef.current;
        if (!mount || editorRef.current) {
            return;
        }

        const editor = monaco.editor.create(mount, {
            value: "",
            language: "plaintext",
            automaticLayout: true,
            minimap: { enabled: false },
            fontFamily: "JetBrains Mono, SFMono-Regular, monospace",
            fontSize: 14,
            scrollBeyondLastLine: false,
            fixedOverflowWidgets: true,
        });
        editorRef.current = editor;

        const contentSubscription = editor.onDidChangeModelContent(() => {
            const model = editor.getModel();
            if (!model) {
                return;
            }
            const path = pathByModelUriRef.current.get(model.uri.toString());
            if (!path) {
                return;
            }
            updateWorkspace((previousWorkspace) => {
                const filesByPath = new Map(previousWorkspace.filesByPath);
                filesByPath.set(path, model.getValue());
                return { ...previousWorkspace, filesByPath };
            });
            scheduleCheck();
        });

        syncEditorWorkspace(workspaceRef.current);

        return () => {
            contentSubscription.dispose();
            editor.dispose();
            editorRef.current = null;
            for (const model of modelByPathRef.current.values()) {
                model.dispose();
            }
            modelByPathRef.current.clear();
            pathByModelUriRef.current.clear();
        };
    }, [scheduleCheck, syncEditorWorkspace, updateWorkspace]);

    useEffect(() => {
        syncEditorWorkspace(workspace);
    }, [syncEditorWorkspace, workspace]);

    useEffect(() => {
        applyDiagnosticMarkers(workspace, diagnostics);
    }, [applyDiagnosticMarkers, diagnostics, workspace]);

    const resetSession = useCallback(async () => {
        if (activeCheckControllerRef.current) {
            activeCheckControllerRef.current.abort();
            activeCheckControllerRef.current = null;
        }
        sessionIdRef.current = null;
        await ensureSession();

        const nextWorkspace = initializeWorkspaceState(
            defaultWorkspaceFiles(),
            "main.bin.copp",
            "main.bin.copp",
        );
        updateWorkspace(() => nextWorkspace);
        setStdout("");
        setStderr("");
        setDiagnostics([]);
        applyDiagnosticMarkers(nextWorkspace, []);
        setStatusText("new session");
        await runCheck();
    }, [applyDiagnosticMarkers, ensureSession, runCheck, updateWorkspace]);

    useEffect(() => {
        void ensureSession()
            .then(() => runCheck())
            .catch((error: unknown) => {
                const message = error instanceof Error
                    ? error.message
                    : String(error);
                setStatusText(`startup error: ${message}`);
            });

        return () => {
            isMountedRef.current = false;
            if (activeCheckControllerRef.current) {
                activeCheckControllerRef.current.abort();
            }
            if (checkTimerRef.current !== null) {
                window.clearTimeout(checkTimerRef.current);
            }
        };
    }, [ensureSession, runCheck]);

    const entrypointPaths = workspace.fileOrder.filter((path) =>
        path.endsWith(".bin.copp")
    );

    return (
        <main className="layout">
            <header className="topbar">
                <h1>Coppice playground</h1>
                <div className="actions">
                    <button type="button" onClick={() => void resetSession()}>
                        New session
                    </button>
                    <button
                        type="button"
                        onClick={() => {
                            const proposedPath = window.prompt(
                                "New file path",
                                "lib/lib.copp",
                            );
                            if (!proposedPath) {
                                return;
                            }

                            const path = proposedPath.trim();
                            if (!path) {
                                return;
                            }
                            if (!path.endsWith(".copp")) {
                                setStatusText("file path must end with .copp");
                                return;
                            }
                            if (path.startsWith("/") || path.includes("..")) {
                                setStatusText(
                                    "file path must be workspace-relative",
                                );
                                return;
                            }
                            if (workspaceRef.current.filesByPath.has(path)) {
                                setStatusText("file already exists");
                                return;
                            }

                            updateWorkspace((previousWorkspace) => {
                                const filesByPath = new Map(
                                    previousWorkspace.filesByPath,
                                );
                                filesByPath.set(path, "");
                                const fileOrder = [
                                    ...previousWorkspace.fileOrder,
                                    path,
                                ];
                                return {
                                    filesByPath,
                                    fileOrder,
                                    activeFilePath: path,
                                    entrypointPath:
                                        previousWorkspace.entrypointPath ??
                                            (path.endsWith(".bin.copp")
                                                ? path
                                                : null),
                                };
                            });
                            scheduleCheck();
                        }}
                    >
                        New file
                    </button>
                    <button
                        type="button"
                        onClick={() => {
                            const state = workspaceRef.current;
                            if (!state.activeFilePath) {
                                return;
                            }
                            if (state.activeFilePath === "PACKAGE.copp") {
                                setStatusText("cannot delete PACKAGE.copp");
                                return;
                            }

                            updateWorkspace((previousWorkspace) => {
                                if (!previousWorkspace.activeFilePath) {
                                    return previousWorkspace;
                                }

                                const filesByPath = new Map(
                                    previousWorkspace.filesByPath,
                                );
                                filesByPath.delete(
                                    previousWorkspace.activeFilePath,
                                );
                                const fileOrder = previousWorkspace.fileOrder
                                    .filter(
                                        (path) =>
                                            path !==
                                                previousWorkspace
                                                    .activeFilePath,
                                    );
                                const binPaths = fileOrder.filter((path) =>
                                    path.endsWith(".bin.copp")
                                );
                                return {
                                    filesByPath,
                                    fileOrder,
                                    activeFilePath: fileOrder[0] ?? null,
                                    entrypointPath:
                                        previousWorkspace.entrypointPath ===
                                                previousWorkspace.activeFilePath
                                            ? (binPaths[0] ?? null)
                                            : previousWorkspace.entrypointPath,
                                };
                            });
                            scheduleCheck();
                        }}
                    >
                        Delete file
                    </button>
                    <button
                        type="button"
                        onClick={() => void runProgram()}
                        disabled={isRunDisabled}
                    >
                        Run
                    </button>
                </div>
            </header>

            <section className="workspace-pane panel">
                <h2>Workspace</h2>
                <label htmlFor="entrypoint">Entrypoint</label>
                <select
                    id="entrypoint"
                    disabled={entrypointPaths.length === 0}
                    value={workspace.entrypointPath ?? ""}
                    onChange={(event) => {
                        const nextEntrypoint = event.currentTarget.value ||
                            null;
                        updateWorkspace((previousWorkspace) => ({
                            ...previousWorkspace,
                            entrypointPath: nextEntrypoint,
                        }));
                        scheduleCheck();
                    }}
                >
                    {entrypointPaths.length === 0
                        ? <option value="">no .bin.copp files</option>
                        : (
                            entrypointPaths.map((path) => (
                                <option key={path} value={path}>
                                    {path}
                                </option>
                            ))
                        )}
                </select>
                <ul id="file-list">
                    {workspace.fileOrder.map((path) => (
                        <li key={path}>
                            <button
                                type="button"
                                className={path === workspace.activeFilePath
                                    ? "file-item active"
                                    : "file-item"}
                                onClick={() => {
                                    updateWorkspace((previousWorkspace) => ({
                                        ...previousWorkspace,
                                        activeFilePath: path,
                                    }));
                                }}
                            >
                                {path}
                            </button>
                        </li>
                    ))}
                </ul>
            </section>

            <section className="editor-pane panel">
                <div className="editor-header">
                    <h2 id="active-file-path">
                        {workspace.activeFilePath ?? "(no file selected)"}
                    </h2>
                </div>
                <div id="editor" ref={editorContainerRef} />
            </section>

            <section className="diagnostics-pane panel">
                <h2>Diagnostics</h2>
                <ul id="diagnostics">
                    {diagnostics.length === 0 ? <li>no diagnostics</li> : (
                        diagnostics.map((diagnostic, index) => (
                            <li
                                key={`${diagnostic.path}-${index}`}
                                className="error"
                            >
                                <button
                                    type="button"
                                    className="diagnostic-link"
                                    onClick={() => {
                                        if (
                                            !workspaceRef.current.filesByPath
                                                .has(
                                                    diagnostic.path,
                                                )
                                        ) {
                                            return;
                                        }
                                        updateWorkspace((
                                            previousWorkspace,
                                        ) => ({
                                            ...previousWorkspace,
                                            activeFilePath: diagnostic.path,
                                        }));
                                    }}
                                >
                                    {`${diagnostic.path}:${diagnostic.span.line}:${diagnostic.span.column} ${diagnostic.message}`}
                                </button>
                            </li>
                        ))
                    )}
                </ul>
            </section>

            <section className="output-pane panel">
                <h2>Program output</h2>
                <pre id="stdout">{stdout}</pre>
                <h2>Program errors</h2>
                <pre id="stderr">{stderr}</pre>
            </section>

            <footer className="status">
                <span id="status">{statusText}</span>
            </footer>
        </main>
    );
}
