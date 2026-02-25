import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import * as monaco from "monaco-editor";

import {
    checkWorkspace,
    createSession,
    type Diagnostic,
    type ExampleSummary,
    listExamples,
    loadExample,
    runWorkspace,
    type WorkspaceFile,
    type WorkspaceRequest,
} from "./api";

const CHECK_DEBOUNCE_MS = 220;
const MARKER_OWNER = "playground-check";
const DEFAULT_EXAMPLE_ID = "hello_world";

type WorkspaceState = {
    filesByPath: Map<string, string>;
    fileOrder: string[];
    activeFilePath: string | null;
    entrypointPath: string | null;
};

type BottomPanelTab = "problems" | "output" | "errors";

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
                [],
                null,
                null,
            ),
        [],
    );

    const [workspace, setWorkspace] = useState<WorkspaceState>(
        initialWorkspace,
    );
    const [diagnostics, setDiagnostics] = useState<Diagnostic[]>([]);
    const [stdout, setStdout] = useState("");
    const [stderr, setStderr] = useState("");
    const [isRunDisabled, setIsRunDisabled] = useState(false);
    const [examples, setExamples] = useState<ExampleSummary[]>([]);
    const [selectedExampleId, setSelectedExampleId] = useState("");
    const [isExampleLoading, setIsExampleLoading] = useState(false);
    const [activeBottomPanelTab, setActiveBottomPanelTab] = useState<
        BottomPanelTab
    >("problems");

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
            return;
        }

        if (activeCheckControllerRef.current) {
            activeCheckControllerRef.current.abort();
        }
        const checkController = new AbortController();
        activeCheckControllerRef.current = checkController;

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
        if (nextDiagnostics.length > 0) {
            setActiveBottomPanelTab("problems");
        }
        if (result.data.error) {
            return;
        }
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
            return;
        }

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
                setActiveBottomPanelTab("errors");
                return;
            }
            if (result.data.timed_out) {
                setActiveBottomPanelTab("errors");
                return;
            }
            if ((result.data.stderr ?? "").trim().length > 0) {
                setActiveBottomPanelTab("errors");
            } else {
                setActiveBottomPanelTab("output");
            }
        } finally {
            if (isMountedRef.current) {
                setIsRunDisabled(false);
            }
        }
    }, [applyDiagnosticMarkers, ensureSession]);

    const loadExampleById = useCallback(async (exampleId: string) => {
        if (!exampleId) {
            return;
        }

        setIsExampleLoading(true);
        try {
            const exampleWorkspace = await loadExample(exampleId);
            const nextWorkspace = initializeWorkspaceState(
                exampleWorkspace.files,
                exampleWorkspace.entrypoint_path,
                exampleWorkspace.entrypoint_path,
            );

            updateWorkspace(() => nextWorkspace);
            setSelectedExampleId(exampleId);
            setStdout("");
            setStderr("");
            setDiagnostics([]);
            setActiveBottomPanelTab("problems");
            applyDiagnosticMarkers(nextWorkspace, []);
            await runCheck();
        } catch (error: unknown) {
        } finally {
            if (isMountedRef.current) {
                setIsExampleLoading(false);
            }
        }
    }, [applyDiagnosticMarkers, runCheck, updateWorkspace]);

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

    useEffect(() => {
        void ensureSession()
            .then(() => runCheck())
            .catch((error: unknown) => {
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

    useEffect(() => {
        void listExamples()
            .then((response) => {
                setExamples(response.examples);
                const defaultExampleId = response.examples.some((example) =>
                        example.id === DEFAULT_EXAMPLE_ID
                    )
                    ? DEFAULT_EXAMPLE_ID
                    : (response.examples[0]?.id ?? "");
                if (!defaultExampleId) {
                    setSelectedExampleId("");
                    return;
                }
                void loadExampleById(defaultExampleId);
            })
            .catch(() => {
                setExamples([]);
                setSelectedExampleId("");
            });
    }, [loadExampleById]);

    const entrypointPaths = workspace.fileOrder.filter((path) =>
        path.endsWith(".bin.copp")
    );
    const actionButtonClassName =
        "rounded-md border border-brand-700 bg-brand-700 px-3 py-2 text-sm font-medium text-white transition hover:bg-brand-800 disabled:cursor-not-allowed disabled:opacity-70";
    const fileActionButtonClassName =
        "rounded-md border border-surface-300 bg-surface-50 px-2.5 py-1.5 text-xs font-medium text-surface-800 transition hover:bg-surface-100 disabled:cursor-not-allowed disabled:opacity-50";
    const activeTabButtonClassName =
        "rounded-md border border-surface-200 bg-surface-0 px-3 py-1.5 text-xs font-medium text-surface-900";
    const inactiveTabButtonClassName =
        "rounded-md border border-transparent px-3 py-1.5 text-xs text-surface-700 transition hover:border-surface-200 hover:bg-surface-100";
    const panelClassName =
        "rounded-xl border border-surface-200 bg-surface-0 p-3";
    const problemsTabLabel = diagnostics.length > 0
        ? `Problems (${diagnostics.length})`
        : "Problems";
    const isDeleteFileDisabled = workspace.activeFilePath === null ||
        workspace.activeFilePath === "PACKAGE.copp";

    return (
        <main className="flex min-h-screen w-full flex-col gap-4 bg-gradient-to-br from-canvas-50 via-canvas-100 to-canvas-200 p-4 text-surface-900 md:h-screen">
            <header className="flex flex-col gap-2 md:flex-row md:items-center md:justify-between">
                <h1 className="text-2xl font-semibold tracking-tight">
                    Coppice playground
                </h1>
                <div className="flex flex-col gap-1 md:items-end">
                    <div className="flex flex-wrap items-center gap-2">
                        <select
                            aria-label="Select example"
                            className="w-52 rounded-md border border-surface-300 bg-surface-0 px-3 py-2 text-sm text-surface-900 disabled:cursor-not-allowed disabled:opacity-50"
                            value={selectedExampleId}
                            onChange={(event) =>
                                void loadExampleById(event.currentTarget.value)}
                            disabled={examples.length === 0 || isExampleLoading}
                        >
                            {examples.length === 0
                                ? <option value="">no examples</option>
                                : examples.map((example) => (
                                    <option key={example.id} value={example.id}>
                                        {example.name}
                                    </option>
                                ))}
                        </select>
                        <button
                            type="button"
                            className={actionButtonClassName}
                            onClick={() => void runProgram()}
                            disabled={isRunDisabled}
                        >
                            Run
                        </button>
                    </div>
                </div>
            </header>

            <div className="grid min-h-0 flex-1 gap-4 md:grid-cols-12">
                <section
                    className={`${panelClassName} md:col-span-3 md:row-span-2`}
                >
                    <h2 className="text-lg font-semibold">Workspace</h2>
                    <label
                        htmlFor="entrypoint"
                        className="mt-3 block text-sm font-medium text-surface-700"
                    >
                        Entrypoint
                    </label>
                    <select
                        id="entrypoint"
                        className="mt-1 w-full rounded-md border border-surface-200 bg-surface-0 px-3 py-2 text-sm text-surface-900 transition focus:border-brand-700 focus:outline-none focus:ring-2 focus:ring-brand-700/20 disabled:cursor-not-allowed disabled:opacity-70"
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
                    <div className="mt-4 flex items-center justify-between gap-2">
                        <h3 className="text-sm font-medium text-surface-700">
                            Files
                        </h3>
                        <div className="flex items-center gap-1.5">
                            <button
                                type="button"
                                className={fileActionButtonClassName}
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
                                        return;
                                    }
                                    if (
                                        path.startsWith("/") ||
                                        path.includes("..")
                                    ) {
                                        return;
                                    }
                                    if (
                                        workspaceRef.current.filesByPath.has(
                                            path,
                                        )
                                    ) {
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
                                            entrypointPath: previousWorkspace
                                                .entrypointPath ??
                                                (path.endsWith(".bin.copp")
                                                    ? path
                                                    : null),
                                        };
                                    });
                                    scheduleCheck();
                                }}
                            >
                                New
                            </button>
                            <button
                                type="button"
                                className={fileActionButtonClassName}
                                disabled={isDeleteFileDisabled}
                                onClick={() => {
                                    const state = workspaceRef.current;
                                    if (!state.activeFilePath) {
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
                                        const fileOrder = previousWorkspace
                                            .fileOrder
                                            .filter(
                                                (path) =>
                                                    path !==
                                                        previousWorkspace
                                                            .activeFilePath,
                                            );
                                        const binPaths = fileOrder.filter((
                                            path,
                                        ) => path.endsWith(".bin.copp"));
                                        return {
                                            filesByPath,
                                            fileOrder,
                                            activeFilePath: fileOrder[0] ??
                                                null,
                                            entrypointPath: previousWorkspace
                                                    .entrypointPath ===
                                                    previousWorkspace
                                                        .activeFilePath
                                                ? (binPaths[0] ?? null)
                                                : previousWorkspace
                                                    .entrypointPath,
                                        };
                                    });
                                    scheduleCheck();
                                }}
                            >
                                Delete
                            </button>
                        </div>
                    </div>
                    <ul className="mt-2 list-none p-0">
                        {workspace.fileOrder.map((path) => (
                            <li
                                key={path}
                                className="mb-1 last:mb-0"
                            >
                                <button
                                    type="button"
                                    className={path === workspace.activeFilePath
                                        ? "w-full border-l-2 border-l-brand-700 bg-brand-50 px-3 py-2.5 text-left text-sm font-medium text-brand-800"
                                        : "w-full border-l-2 border-l-transparent bg-transparent px-3 py-2.5 text-left text-sm text-surface-700 transition hover:bg-surface-100 hover:text-surface-900"}
                                    onClick={() => {
                                        updateWorkspace((
                                            previousWorkspace,
                                        ) => ({
                                            ...previousWorkspace,
                                            activeFilePath: path,
                                        }));
                                    }}
                                >
                                    <span className="inline-flex items-center gap-2">
                                        <span
                                            aria-hidden="true"
                                            className={path ===
                                                    workspace.activeFilePath
                                                ? "h-3 w-3 rounded-sm border border-brand-700 bg-brand-100"
                                                : "h-3 w-3 rounded-sm border border-surface-400 bg-transparent"}
                                        />
                                        <span>{path}</span>
                                    </span>
                                </button>
                            </li>
                        ))}
                    </ul>
                </section>

                <section
                    className={`${panelClassName} flex min-h-0 flex-col md:col-span-9`}
                >
                    <h2 className="mb-2 text-lg font-semibold">
                        {workspace.activeFilePath ?? "(no file selected)"}
                    </h2>
                    <div
                        id="editor"
                        className="h-full min-h-80 w-full overflow-hidden rounded-lg border border-surface-200 md:min-h-0"
                        ref={editorContainerRef}
                    />
                </section>

                <section
                    className={`${panelClassName} flex h-56 min-h-0 flex-col md:col-span-9`}
                >
                    <div className="mb-3 flex items-center gap-2 border-b border-surface-200 pb-2">
                        <button
                            type="button"
                            className={activeBottomPanelTab === "problems"
                                ? activeTabButtonClassName
                                : inactiveTabButtonClassName}
                            onClick={() => setActiveBottomPanelTab("problems")}
                        >
                            {problemsTabLabel}
                        </button>
                        <button
                            type="button"
                            className={activeBottomPanelTab === "output"
                                ? activeTabButtonClassName
                                : inactiveTabButtonClassName}
                            onClick={() => setActiveBottomPanelTab("output")}
                        >
                            Output
                        </button>
                        <button
                            type="button"
                            className={activeBottomPanelTab === "errors"
                                ? activeTabButtonClassName
                                : inactiveTabButtonClassName}
                            onClick={() => setActiveBottomPanelTab("errors")}
                        >
                            Errors
                        </button>
                    </div>
                    <div className="min-h-0 flex-1 overflow-auto">
                        {activeBottomPanelTab === "problems"
                            ? (
                                <ul className="list-none p-0">
                                    {diagnostics.length === 0
                                        ? (
                                            <li className="py-1 text-sm text-surface-700">
                                                no diagnostics
                                            </li>
                                        )
                                        : (
                                            diagnostics.map((
                                                diagnostic,
                                                index,
                                            ) => (
                                                <li
                                                    key={`${diagnostic.path}-${index}`}
                                                    className="border-b border-surface-200 py-1 last:border-b-0"
                                                >
                                                    <button
                                                        type="button"
                                                        className="w-full rounded-md border border-transparent bg-transparent px-1 py-1 text-left text-sm text-danger-700 hover:bg-danger-50"
                                                        onClick={() => {
                                                            if (
                                                                !workspaceRef
                                                                    .current
                                                                    .filesByPath
                                                                    .has(
                                                                        diagnostic
                                                                            .path,
                                                                    )
                                                            ) {
                                                                return;
                                                            }
                                                            updateWorkspace((
                                                                previousWorkspace,
                                                            ) => ({
                                                                ...previousWorkspace,
                                                                activeFilePath:
                                                                    diagnostic
                                                                        .path,
                                                            }));
                                                        }}
                                                    >
                                                        {`${diagnostic.path}:${diagnostic.span.line}:${diagnostic.span.column} ${diagnostic.message}`}
                                                    </button>
                                                </li>
                                            ))
                                        )}
                                </ul>
                            )
                            : activeBottomPanelTab === "output"
                            ? (
                                <pre className="m-0 min-h-full whitespace-pre-wrap rounded-lg bg-surface-50 p-2 font-mono text-sm">
                                    {stdout || "(no output)"}
                                </pre>
                            )
                            : (
                                <pre className="m-0 min-h-full whitespace-pre-wrap rounded-lg bg-surface-50 p-2 font-mono text-sm">
                                    {stderr || "(no errors)"}
                                </pre>
                            )}
                    </div>
                </section>
            </div>
        </main>
    );
}
