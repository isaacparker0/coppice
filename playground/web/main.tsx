import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./app";
import "./style.css";

const appElement = document.querySelector<HTMLDivElement>("#app");
if (!appElement) {
    throw new Error("missing #app mount element");
}

const root = createRoot(appElement);
root.render(
    <StrictMode>
        <App />
    </StrictMode>,
);
