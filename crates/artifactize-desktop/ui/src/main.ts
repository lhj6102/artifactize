import { mount } from "svelte";
import App from "./App.svelte";
import "./app.css";

if (import.meta.env.MODE === "e2e") await import("@wdio/tauri-plugin");

mount(App, { target: document.getElementById("app") as HTMLElement });
