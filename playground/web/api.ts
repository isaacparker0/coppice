export type WorkspaceFile = {
    path: string;
    source: string;
};

export type WorkspaceRequest = {
    entrypoint_path: string | null;
    files: WorkspaceFile[];
};

type SessionResponse = {
    session_id: string;
};

export type Diagnostic = {
    path: string;
    message: string;
    span: {
        start: number;
        end: number;
        line: number;
        column: number;
    };
};

type CheckOrRunError = {
    message: string;
};

export type CheckResponse = {
    ok?: boolean;
    diagnostics?: Diagnostic[];
    error?: CheckOrRunError;
};

export type RunResponse = CheckResponse & {
    stdout?: string;
    stderr?: string;
    timed_out?: boolean;
    exit_code?: number;
};

async function postJson<TResponse>(
    path: string,
    body: unknown,
    signal?: AbortSignal,
): Promise<{ status: number; data: TResponse }> {
    const response = await fetch(path, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(body),
        signal,
    });
    const data = (await response.json()) as TResponse;
    return { status: response.status, data };
}

export async function createSession(): Promise<SessionResponse> {
    const response = await postJson<SessionResponse>("/session", {});
    if (response.status < 200 || response.status >= 300) {
        throw new Error(`session request failed: ${response.status}`);
    }
    return response.data;
}

export function checkWorkspace(
    sessionId: string,
    workspaceRequest: WorkspaceRequest,
    signal: AbortSignal,
): Promise<{ status: number; data: CheckResponse }> {
    return postJson<CheckResponse>(
        "/check",
        {
            session_id: sessionId,
            entrypoint_path: workspaceRequest.entrypoint_path,
            files: workspaceRequest.files,
        },
        signal,
    );
}

export function runWorkspace(
    sessionId: string,
    workspaceRequest: WorkspaceRequest,
): Promise<{ status: number; data: RunResponse }> {
    return postJson<RunResponse>("/run", {
        session_id: sessionId,
        entrypoint_path: workspaceRequest.entrypoint_path,
        files: workspaceRequest.files,
    });
}
