let sessionId = null;
let debounceTimer = null;
let activeCheckController = null;

const editor = document.getElementById("editor");
const runButton = document.getElementById("run");
const newSessionButton = document.getElementById("new-session");
const stdoutElement = document.getElementById("stdout");
const stderrElement = document.getElementById("stderr");

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
    const id = await ensureSession();
    if (activeCheckController) {
        activeCheckController.abort();
    }
    activeCheckController = new AbortController();

    playgroundEditor.setStatus("checking...");
    const result = await checkSource(
        id,
        editor.value,
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

    if (result.data.error) {
        playgroundEditor.renderDiagnostics([
            {
                line: 1,
                column: 1,
                message: result.data.error.message,
                phase: result.data.error.kind,
            },
        ]);
        playgroundEditor.setStatus("check failed");
        return;
    }

    playgroundEditor.renderDiagnostics(result.data.diagnostics);
    playgroundEditor.setStatus(result.data.ok ? "ok" : "diagnostics");
}

async function runProgram() {
    const id = await ensureSession();
    playgroundEditor.setStatus("running...");
    runButton.disabled = true;

    try {
        const result = await runSource(id, editor.value);
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
    sessionId = null;
    await ensureSession();
    stdoutElement.textContent = "";
    stderrElement.textContent = "";
    playgroundEditor.renderDiagnostics([]);
    playgroundEditor.setStatus("new session");
}

editor.value = playgroundEditor.defaultProgram();
void ensureSession().then(() => runCheck());
editor.addEventListener("input", scheduleCheck);
runButton.addEventListener("click", () => void runProgram());
newSessionButton.addEventListener("click", () => void resetSession());
