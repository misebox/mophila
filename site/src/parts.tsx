import { For, Index, Show, createEffect, createSignal, on, untrack, type Component, type JSX } from "solid-js";
import { marked, type Tokens } from "marked";
import { Heading, Link } from "@/components/ui";
import { data } from "@/data";
import { href, route } from "@/route";

/** 文中の型名を、その型のページへのリンクにする。組み込みの型として載っている名前だけ */
export const TypeText: Component<{ text: string }> = (props) => (
  <Index each={props.text.split(/([A-Za-z_][A-Za-z0-9_]*)/)}>
    {(part) => (
      <Show when={data.types.some((t) => t.name === part())} fallback={part()}>
        <a href={href("builtins", part())}>{part()}</a>
      </Show>
    )}
  </Index>
);

export const Code: Component<{ text: string }> = (props) => (
  <pre class="code"><code>{props.text}</code></pre>
);

// 見出しに id を付けて、サイドバーから飛べるようにする
export const slug = (s: string): string => s.trim().toLowerCase().replace(/[^\p{L}\p{N}]+/gu, "-").replace(/^-|-$/g, "");

marked.use({
  renderer: {
    heading(this: { parser: { parseInline(tokens: Tokens.Heading["tokens"]): string } }, token: Tokens.Heading): string {
      return `<h${token.depth} id="${slug(token.text)}">${this.parser.parseInline(token.tokens)}</h${token.depth}>\n`;
    },
  },
});

export interface TocEntry { id: string; text: string; depth: number }

export const tocOf = (markdown: string): TocEntry[] =>
  marked.lexer(markdown).flatMap((t) => (t.type === "heading" && t.depth >= 2 && t.depth <= 3 ? [{ id: slug(t.text), text: t.text, depth: t.depth }] : []));

export const Markdown: Component<{ text: string }> = (props) => {
  const html = (): string => String(marked.parse(props.text));
  return <div class="md" innerHTML={html()} />;
};

/** 鍵ごとにまとめる。鍵は最初に現れた順 */
export const groupBy = <T,>(items: T[], key: (item: T) => string): [string, T[]][] => {
  const groups: [string, T[]][] = [];
  for (const it of items) {
    const k = key(it);
    const found = groups.find(([g]) => g === k);
    if (found) found[1].push(it);
    else groups.push([k, [it]]);
  }
  return groups;
};

export interface IndexItem { label: string; href: string; active?: boolean; depth?: number }
export interface IndexGroup { title?: string; href?: string; active?: boolean; items: IndexItem[] }

// 題のある group は畳める。中の項目が選ばれたら開く
const Caret: Component<{ open: boolean }> = (props) => (
  <span class="side-caret" classList={{ open: props.open }} aria-hidden="true">▸</span>
);

// 開いている group を題で覚える。既定は閉じていて、今いる項目を含む group だけ開く
const [opened, setOpened] = createSignal<Record<string, boolean>>({});

// 題のある group は畳める。題がページでもある (ライブラリの module) ときは、
// 題を押すとそのページ、右の三角で開閉する
const SideGroup: Component<{ group: IndexGroup }> = (props) => {
  const key = (): string => props.group.title ?? "";
  const hasActive = (): boolean => props.group.items.some((it) => it.active === true);
  const open = (): boolean => hasActive() || opened()[key()] === true;
  const toggle = (): void => {
    setOpened({ ...opened(), [key()]: !open() });
  };
  return (
    <div class="side-group">
      <Show when={props.group.title}>
        <Show
          when={props.group.href}
          fallback={
            <Show when={props.group.items.length > 0} fallback={<div class="side-head"><span class="side-title">{props.group.title}</span></div>}>
              <button type="button" class="side-head side-head-btn" aria-expanded={open()} onClick={toggle}>
                <span class="side-title">{props.group.title}</span>
                <Caret open={open()} />
              </button>
            </Show>
          }
        >
          {(h) => (
            <div class="side-head">
              <a href={h()} class="side-title side-title-link" classList={{ active: props.group.active === true }}>{props.group.title}</a>
              <Show when={props.group.items.length > 0}>
                <button type="button" class="side-caret-btn" aria-expanded={open()} aria-label={`${props.group.title} の中身を開閉`} onClick={toggle}>
                  <Caret open={open()} />
                </button>
              </Show>
            </div>
          )}
        </Show>
      </Show>
      <Show when={open() || !props.group.title}>
        <div class="side-items" classList={{ nested: !!props.group.title }}>
          <For each={props.group.items}>
            {(it) => (
              <a href={it.href} class="side-link" classList={{ active: it.active === true, deep: (it.depth ?? 0) > 2 }}>{it.label}</a>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
};

// 左に置く目次
export const SideIndex: Component<{ groups: IndexGroup[] }> = (props) => (
  <nav aria-label="目次">
    <For each={props.groups}>{(g) => <SideGroup group={g} />}</For>
  </nav>
);

// 狭い画面では目次を画面いっぱいのパネルにし、選んだら閉じる。上のバーに今の選択を出す
export const WithSide: Component<{ side: JSX.Element; current: string; children: JSX.Element }> = (props) => {
  const [open, setOpen] = createSignal(false);
  createEffect(on(route, () => setOpen(false), { defer: true }));
  createEffect(() => {
    document.body.classList.toggle("index-open", open());
  });
  return (
    <div class="with-side">
      <div class="side-bar">
        <button type="button" class="side-toggle" aria-expanded={open()} onClick={() => setOpen(true)}>
          <span class="side-current">{props.current}</span>
          <span class="side-open-label">目次</span>
        </button>
      </div>
      <div class="side" classList={{ open: open() }}>
        <div class="side-sheet-bar">
          <span>目次</span>
          <button type="button" class="side-close" onClick={() => setOpen(false)}>閉じる</button>
        </div>
        <div class="side-scroll">{props.side}</div>
      </div>
      <div class="main">{props.children}</div>
    </div>
  );
};

export const Section: Component<{ id: string; title: string; children: JSX.Element }> = (props) => (
  <section id={props.id} class="section">
    <Heading level={2} size="lg">{props.title}</Heading>
    {props.children}
  </section>
);

export const GitHubLink: Component<{ href: string; label?: string }> = (props) => (
  <Link href={props.href} external underline="hover" tone="neutral" class="gh">{props.label ?? "GitHub で見る"}</Link>
);



/** key が変わったら一度消してから、新しい中身を出す */
export const Faded: Component<{ key: string; children: (key: string) => JSX.Element }> = (props) => {
  const [shown, setShown] = createSignal(props.key);
  const [out, setOut] = createSignal(false);
  createEffect(on(() => props.key, (next) => {
    if (next === untrack(shown)) return;
    setOut(true);
    setTimeout(() => {
      setShown(next);
      setOut(false);
    }, 150);
  }, { defer: true }));
  return <div class="fade" classList={{ out: out() }}>{props.children(shown())}</div>;
};
