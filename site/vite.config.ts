import { execSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

// GitHub のリポジトリ URL。Actions では GITHUB_REPOSITORY、手元では git の origin から
function repoUrl(): string {
  const gh = process.env.GITHUB_REPOSITORY;
  if (gh) return `https://github.com/${gh}`;
  const origin = execSync("git remote get-url origin", { encoding: "utf8" }).trim();
  const m = origin.match(/github\.com[:/](.+?)(?:\.git)?$/);
  if (!m) throw new Error(`origin が GitHub ではない: ${origin}`);
  return `https://github.com/${m[1]}`;
}

// base は相対。GitHub Pages のサブパス (/<repo>/) でもそのまま動く
export default defineConfig({
  plugins: [solid()],
  base: "./",
  define: { __REPO_URL__: JSON.stringify(repoUrl()) },
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
});
