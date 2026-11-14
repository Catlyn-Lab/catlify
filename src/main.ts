import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";
import { initPlatform } from "./lib/platform";

initPlatform();

if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window && (window as any).__TAURI_INTERNALS__?.invoke)) {
  (await import("./lib/mock")).installMocks();
}

export default mount(App, { target: document.getElementById("app")! });
