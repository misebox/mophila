import { For, Show, type Component } from "solid-js";
import { Heading, Stack, Text } from "@/components/ui";
import { data, type Doc } from "@/data";
import { Code, GitHubLink, Markdown, SideIndex, WithSide, tocOf, type IndexGroup } from "@/parts";
import { href, route } from "@/route";
import { blob } from "@/repo";

const EXAMPLES = "examples";
// エディタの説明は「使い方」に置くので、ここには出さない
const DOCS = data.docs.filter((d) => d.key !== "editors");
const exampleId = (name: string): string => name.replace(/\.moph$/, "");

// #/docs/<鍵>/<見出し>
const currentKey = (): string => route().sub.split("/")[0] || DOCS[0].key;
const anchor = (): string => route().sub.split("/").slice(1).join("/");
const currentTitle = (): string => (currentKey() === EXAMPLES ? "構文の例" : currentDoc()?.title ?? "");
const currentDoc = (): Doc | undefined => DOCS.find((d) => d.key === currentKey());

// 目次: 文書ごとに、その中の見出し
const groups = (): IndexGroup[] => {
  const key = currentKey();
  const at = (k: string, id: string): boolean => key === k && anchor() === id;
  return [
    ...DOCS.map((d) => ({
      title: d.title,
      href: href("docs", d.key),
      active: d.key === key,
      items: d.path.endsWith(".md")
        ? tocOf(d.text).map((h) => ({ label: h.text, href: href("docs", `${d.key}/${h.id}`), depth: h.depth, active: at(d.key, h.id) }))
        : [],
    })),
    {
      title: "構文の例",
      href: href("docs", EXAMPLES),
      active: key === EXAMPLES,
      items: data.examples.map((e) => ({ label: exampleId(e.name), href: href("docs", `${EXAMPLES}/${exampleId(e.name)}`), active: at(EXAMPLES, exampleId(e.name)) })),
    },
  ];
};

const DocBody: Component<{ doc: Doc }> = (props) => (
  <Stack gap={3}>
    <Show when={props.doc.path.endsWith(".md")} fallback={<><Heading level={1} size="xl">{props.doc.title}</Heading><Code text={props.doc.text} /></>}>
      <Markdown text={props.doc.text} />
    </Show>
    <GitHubLink href={blob(props.doc.path)} />
  </Stack>
);

const Examples: Component = () => (
  <Stack gap={5}>
    <Stack gap={1}>
      <Heading level={1} size="xl">構文の例</Heading>
      <Text size="sm" tone="muted" class="intro">構文ごとの短いスクリプト。</Text>
    </Stack>
    <For each={data.examples}>
      {(e) => (
        <Stack gap={2} id={exampleId(e.name)} class="entry">
          <Heading level={2} size="md">{exampleId(e.name)}</Heading>
          <Code text={e.code} />
          <GitHubLink href={blob(`examples/${e.name}`)} />
        </Stack>
      )}
    </For>
  </Stack>
);

export const Docs: Component = () => (
  <WithSide current={currentTitle()} side={<SideIndex groups={groups()} />}>
    <Show when={currentKey() === EXAMPLES} fallback={<Show when={currentDoc()} fallback={<Text tone="muted">文書がありません</Text>}>{(d) => <DocBody doc={d()} />}</Show>}>
      <Examples />
    </Show>
  </WithSide>
);
