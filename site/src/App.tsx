import { Switch, Match, createEffect, on, type Component, type JSX } from "solid-js";
import { Container } from "@/components/ui";
import { href, route } from "@/route";
import { repo } from "@/repo";
import { Overview } from "@/pages/Overview";
import { Start } from "@/pages/Start";
import { Samples } from "@/pages/Samples";
import { Builtins } from "@/pages/Builtins";
import { Library } from "@/pages/Library";
import { Docs } from "@/pages/Docs";

const PAGES: [string, string][] = [
  ["", "概要"],
  ["start", "使い方"],
  ["samples", "サンプル"],
  ["builtins", "組み込み"],
  ["lib", "ライブラリ"],
  ["docs", "仕様と文書"],
];

// 見出しの id。文書は #/docs/<鍵>/<見出し>、それ以外は sub がそのまま id
const anchorOf = (): string => {
  const r = route();
  if (r.page === "samples") return "";
  if (r.page === "docs") return r.sub.split("/").slice(1).join("/");
  return r.sub;
};

export const App: Component = (): JSX.Element => {
  createEffect(on(route, () => {
    const id = anchorOf();
    requestAnimationFrame(() => {
      const el = id ? document.getElementById(id) : null;
      if (el) el.scrollIntoView({ block: "start" });
      else window.scrollTo(0, 0);
    });
  }));
  return (
    <>
      <header class="top">
        <Container size="xl" padded>
          <div class="top-row">
            <a href={href("")} class="brand">mophila</a>
            <nav class="top-nav" aria-label="ページ">
              {PAGES.map(([page, label]) => (
                <a href={href(page)} classList={{ active: route().page === page }}>{label}</a>
              ))}
            </nav>
            <a href={repo} class="top-gh" target="_blank" rel="noopener noreferrer">GitHub</a>
          </div>
        </Container>
      </header>
      <main>
        <Container size="xl" padded>
          <Switch fallback={<Overview />}>
            <Match when={route().page === "start"}><Start /></Match>
            <Match when={route().page === "samples"}><Samples /></Match>
            <Match when={route().page === "builtins"}><Builtins /></Match>
            <Match when={route().page === "lib"}><Library /></Match>
            <Match when={route().page === "docs"}><Docs /></Match>
          </Switch>
        </Container>
      </main>
    </>
  );
};
