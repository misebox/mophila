import { For, Show, type Component, type JSX } from "solid-js";
import { marked, type Tokens } from "marked";
import { Heading, Link, Text } from "@/components/ui";

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

export interface IndexItem { label: string; href: string; active?: boolean; depth?: number }
export interface IndexGroup { title?: string; items: IndexItem[] }

// 左に置く目次
export const SideIndex: Component<{ groups: IndexGroup[] }> = (props) => (
  <nav class="side" aria-label="目次">
    <For each={props.groups}>
      {(g) => (
        <div class="side-group">
          <Show when={g.title}><div class="side-title">{g.title}</div></Show>
          <For each={g.items}>
            {(it) => (
              <a href={it.href} class="side-link" classList={{ active: it.active === true, deep: (it.depth ?? 0) > 2 }}>{it.label}</a>
            )}
          </For>
        </div>
      )}
    </For>
  </nav>
);

export const WithSide: Component<{ side: JSX.Element; children: JSX.Element }> = (props) => (
  <div class="with-side">
    {props.side}
    <div class="main">{props.children}</div>
  </div>
);

export const Section: Component<{ id: string; title: string; children: JSX.Element }> = (props) => (
  <section id={props.id} class="section">
    <Heading level={2} size="lg">{props.title}</Heading>
    {props.children}
  </section>
);

export const GitHubLink: Component<{ href: string; label?: string }> = (props) => (
  <Link href={props.href} external underline="hover" tone="neutral" class="gh">{props.label ?? "GitHub で見る"}</Link>
);

export const Lead: Component<{ children: JSX.Element }> = (props) => (
  <Text tone="muted" class="lead">{props.children}</Text>
);
