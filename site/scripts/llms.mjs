// ドキュメントの元データを作り直してから、llms.txt と llms-full.txt を書く。
// build の前に走るので、手で回す順番を覚える必要はない。
//
//   1. cargo build + scripts/docgen.py  → src/data.json (Rust があるときだけ)
//   2. data.json とリポジトリの md / gbnf → public/llms.txt, public/llms-full.txt
//
// CI には Rust が無いので 1 は飛ばし、コミット済みの data.json を使う。
// ここには本文を書かない (すべて読み込んだもの)。
import { execSync, spawnSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const site = join(here, "..");
const root = join(site, "..");
const read = (p) => readFileSync(join(root, p), "utf8");

// 元データを作り直す。動画 (--media) は GPU が要るので、ここでは作らない
function refresh() {
  const has = (cmd) => spawnSync(cmd, ["--version"], { stdio: "ignore" }).status === 0;
  if (!has("cargo") || !has("python3")) {
    console.log("cargo か python3 が無いので data.json はコミット済みのものを使う");
    return;
  }
  execSync("cargo build --quiet", { cwd: root, stdio: "inherit" });
  execSync("python3 scripts/docgen.py", { cwd: root, stdio: "inherit" });
}

refresh();

// owner/name。Actions では GITHUB_REPOSITORY、手元では git の origin から
function repoPath() {
  if (process.env.GITHUB_REPOSITORY) return process.env.GITHUB_REPOSITORY;
  const origin = execSync("git remote get-url origin", { encoding: "utf8" }).trim();
  const m = origin.match(/github\.com[:/](.+?)(?:\.git)?$/);
  if (!m) throw new Error(`origin が GitHub ではない: ${origin}`);
  return m[1];
}

const [owner, name] = repoPath().split("/");
const pages = `https://${owner}.github.io/${name}`;
const repo = `https://github.com/${owner}/${name}`;
const raw = `https://raw.githubusercontent.com/${owner}/${name}/main`;

const data = JSON.parse(readFileSync(join(site, "src", "data.json"), "utf8"));
// 1 行の説明は README を正とする
const summary = read("README.md").split("\n").slice(1).map((l) => l.trim()).find((l) => l && !l.startsWith("#"));

const shapes = data.types.filter((t) => t.category === "Shape").map((t) => t.name).join(", ");
const modules = data.libs.map((l) => l.name).join(", ");

const index = `# ${name}

> ${summary}

${name} を書くときは llms-full.txt を読む。文法は GBNF にしてあるので、生成を制約できるなら使う。

- 図形: ${shapes}
- module: ${modules}

## 読むもの

- [llms-full.txt](${pages}/llms-full.txt): 下に挙げたものを 1 ファイルにまとめたもの。まずこれを読む
- [書き方の要点](${raw}/docs/skills/mophila/SKILL.md): 見やすい動画にするための書き方と、つまずきやすい点
- [文法 (GBNF)](${raw}/docs/mophila.gbnf): 構文の全部。文法で生成を制約するときに渡す
- [言語仕様](${raw}/docs/mophila-spec.md): 意味と規則

## 参照

- [builtin と module](${pages}/#/builtins): 型・属性・メソッド・エラーの一覧
- [サンプル](${pages}/#/samples): 動く .moph と、その動画
- [リポジトリ](${repo}): ソースと examples/
`;

/** 取り込む文書の見出しを 1 段下げる。囲みの中の # は見出しではないので触らない */
function demote(md) {
  let fence = false;
  return md
    .split("\n")
    .map((line) => {
      if (line.startsWith("```")) fence = !fence;
      return fence || !line.startsWith("#") ? line : `#${line}`;
    })
    .join("\n");
}

const section = (title, body) => `\n\n# ${title}\n\n${demote(body).trim()}\n`;

function builtinText() {
  const out = ["import なしで呼べる関数:", ""];
  out.push(...data.builtins.map((e) => `- \`${e.signature}\` -> ${e.returns} — ${e.doc}`));
  for (const t of data.types) {
    out.push("", `## ${t.name}`, "", t.doc);
    if (t.make) out.push("", "作り方:", "", "```", t.make, "```");
    if (t.values.length) out.push("", "値:", "", ...t.values.map((v) => `- \`${v.value}\` — ${v.doc}`));
    if (t.members.length) out.push("", `まとめている型: ${t.members.join(" | ")}`);
    if (t.attrs.length) out.push("", "属性:", "", ...t.attrs.map((a) => `- \`${a.name}\`: ${a.type} — ${a.doc}`));
    if (t.methods.length) out.push("", "メソッド:", "", ...t.methods.map((m) => `- \`${m.signature}\`${m.returns ? ` -> ${m.returns}` : ""} — ${m.doc}`));
  }
  out.push("", "## エラー", "", ...data.errors.map((e) => `- \`${e.name}\` — ${e.doc}`));
  return out.join("\n");
}

function moduleText() {
  return data.libs
    .map((lib) =>
      [
        `## ${lib.name}`,
        "",
        ...lib.entries.map((e) => `- \`${e.signature}\` -> ${e.returns} — ${e.doc}`),
        ...lib.items.map((it) => `- \`${it.call}\`${it.returns.type ? ` -> ${it.returns.type}` : ""} — ${it.summary}`),
        "",
      ].join("\n"),
    )
    .join("\n");
}

let full = `# ${name}\n\n${summary}\n\nこの 1 ファイルに、書き方・文法・仕様・builtin と module の一覧・構文の例が入っている。`;
full += section("書き方の要点", read("docs/skills/mophila/SKILL.md"));
full += section("文法 (GBNF)", "```\n" + read("docs/mophila.gbnf").trim() + "\n```");
full += section("言語仕様", read("docs/mophila-spec.md"));
full += section("builtin の型", builtinText());
full += section("module", moduleText());
full += section("構文の例", data.examples.map((e) => `## ${e.name}\n\n\`\`\`\n${e.code.trim()}\n\`\`\``).join("\n\n"));

writeFileSync(join(site, "public", "llms.txt"), index);
writeFileSync(join(site, "public", "llms-full.txt"), full);
console.log(`public/llms.txt: ${index.length} 文字, public/llms-full.txt: ${full.length} 文字`);
