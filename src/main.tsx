import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";

import "./styles/base.css";
import "./styles/shell.css";
import "./styles/components.css";
import "./styles/chat.css";

const host = document.getElementById("root");
if (!host) {
  throw new Error("missing #root element");
}

createRoot(host).render(
  <StrictMode>
    <App />
  </StrictMode>,
);