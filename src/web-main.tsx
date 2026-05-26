/**
 * Web entry point — same React app but with VITE_WEB_MODE=1.
 * Loads from web.html which references this file.
 */
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles/index.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
