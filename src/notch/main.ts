import { mount } from "svelte";
import Notch from "./Notch.svelte";

if (import.meta.env.DEV && !(window as any).__TAURI_INTERNALS__?.invoke) {
  (await import("./mock")).installNotchMocks();
}

export default mount(Notch, { target: document.getElementById("notch")! });
