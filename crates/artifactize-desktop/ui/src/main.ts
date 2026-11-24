import { mount } from "svelte";
import App from "./App.svelte";
import PetWindow from "./pet/PetWindow.svelte";
import ViewerWindow from "./viewers/ViewerWindow.svelte";
import "./app.css";

if (import.meta.env.MODE === "e2e") await import("@wdio/tauri-plugin");

// One bundle, three kinds of window: the main window, an Artifact Viewer (`?viewer=SESSION`) and
// the pet overlay (`?pet=REQUEST`), as the backend opens them.
const query = new URLSearchParams(location.search);
const target = document.getElementById("app") as HTMLElement;
const viewer = query.get("viewer");
const pet = query.get("pet");
if (viewer !== null) mount(ViewerWindow, { target, props: { session: Number(viewer) } });
else if (pet !== null) mount(PetWindow, { target, props: { request: pet } });
else mount(App, { target });
