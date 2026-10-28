import { getCurrentWindow } from "@tauri-apps/api/window";
import { mount } from "svelte";
import App from "./App.svelte";
import LoadingBox from "./LoadingBox.svelte";
import "./styles/vendor/xp.css-0.2.6/XP.css";
import "./styles/app.css";

// Both windows load this page; the label says which one this is.
const component = getCurrentWindow().label === "loading" ? LoadingBox : App;
mount(component, { target: document.getElementById("app")! });
