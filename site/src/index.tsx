import { render } from "solid-js/web";
import "./soluid.css";
import "./styles.css";
import { App } from "./App";

// 配色は OS の設定に合わせる
const dark = window.matchMedia("(prefers-color-scheme: dark)");
const applyTheme = (): void => document.documentElement.setAttribute("data-theme", dark.matches ? "dark" : "light");
applyTheme();
dark.addEventListener("change", applyTheme);

const root = document.getElementById("root");
if (!root) throw new Error("#root がない");
render(() => <App />, root);
