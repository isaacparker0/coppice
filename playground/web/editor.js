function defaultProgram() {
    return `function main() -> nil {
    print("hello, world")
    return nil
}`;
}

function renderDiagnostics(diagnostics) {
    const diagnosticsElement = document.getElementById("diagnostics");
    diagnosticsElement.innerHTML = "";

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
            `${diagnostic.line}:${diagnostic.column} ${diagnostic.message} (${diagnostic.phase})`;
        diagnosticsElement.appendChild(row);
    }
}

function setStatus(text) {
    document.getElementById("status").textContent = text;
}

window.playgroundEditor = {
    defaultProgram,
    renderDiagnostics,
    setStatus,
};
