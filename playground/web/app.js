let sessionId = null;
let debounceTimer = null;
let activeCheckController = null;
let editorReady = null;

const runButton = document.getElementById("run");
const newSessionButton = document.getElementById("new-session");
const stdoutElement = document.getElementById("stdout");
const stderrElement = document.getElementById("stderr");

function ensureEditorReady() {
    if (!editorReady) {
        editorReady = playgroundEditor.initEditor({
            mount: document.getElementById("editor"),
            onChange: scheduleCheck,
        });
    }
    return editorReady;
}

async function ensureSession() {
    if (sessionId) {
        return sessionId;
    }
    const session = await createSession();
    sessionId = session.session_id;
    return sessionId;
}

function scheduleCheck() {
    clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
        void runCheck();
    }, 220);
}

async function runCheck() {
    await ensureEditorReady();
    const id = await ensureSession();
    if (activeCheckController) {
        activeCheckController.abort();
    }
    activeCheckController = new AbortController();

    playgroundEditor.setStatus("checking...");
    const result = await checkSource(
        id,
        playgroundEditor.getSource(),
        activeCheckController.signal,
    ).catch((error) => {
        if (error.name === "AbortError") {
            return null;
        }
        throw error;
    });
    if (!result) {
        return;
    }

    playgroundEditor.renderDiagnostics(result.data.diagnostics);
    if (result.data.error) {
        playgroundEditor.setStatus("check failed");
        return;
    }
    playgroundEditor.setStatus(result.data.ok ? "ok" : "diagnostics");
}

async function runProgram() {
    await ensureEditorReady();
    const id = await ensureSession();
    playgroundEditor.setStatus("running...");
    runButton.disabled = true;

    try {
        const result = await runSource(id, playgroundEditor.getSource());
        playgroundEditor.renderDiagnostics(result.data.diagnostics || []);

        stdoutElement.textContent = result.data.stdout || "";
        stderrElement.textContent = result.data.stderr || "";

        if (result.data.error) {
            playgroundEditor.setStatus(
                `run error: ${result.data.error.message}`,
            );
            return;
        }

        if (result.data.timed_out) {
            playgroundEditor.setStatus("run timed out");
            return;
        }

        playgroundEditor.setStatus(`exit code ${result.data.exit_code}`);
    } finally {
        runButton.disabled = false;
    }
}

async function resetSession() {
    await ensureEditorReady();
    if (activeCheckController) {
        activeCheckController.abort();
        activeCheckController = null;
    }
    sessionId = null;
    await ensureSession();
    playgroundEditor.setSource(playgroundEditor.defaultProgram());
    stdoutElement.textContent = "";
    stderrElement.textContent = "";
    playgroundEditor.renderDiagnostics([]);
    playgroundEditor.setStatus("new session");
    await runCheck();
}

runButton.addEventListener("click", () => void runProgram());
newSessionButton.addEventListener("click", () => void resetSession());

void ensureEditorReady().then(() => ensureSession()).then(() => runCheck());
