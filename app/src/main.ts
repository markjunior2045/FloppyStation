import { mount } from "svelte";
import App from "./App.svelte";
import "./styles/vendor/xp.css-0.2.6/XP.css";
import "./styles/app.css";

mount(App, { target: document.getElementById("app")! });
