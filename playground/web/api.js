async function createSession() {
    const response = await fetch("/session", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: "{}",
    });
    if (!response.ok) {
        throw new Error(`session request failed: ${response.status}`);
    }
    return response.json();
}

async function checkSource(sessionId, source, signal) {
    const response = await fetch("/check", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ session_id: sessionId, source }),
        signal,
    });
    const data = await response.json();
    return { status: response.status, data };
}

async function runSource(sessionId, source) {
    const response = await fetch("/run", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ session_id: sessionId, source }),
    });
    const data = await response.json();
    return { status: response.status, data };
}
