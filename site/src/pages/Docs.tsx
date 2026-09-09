import { For, Show, type Component } from "solid-js";
import { Heading, Stack, Text } from "@/components/ui";
import { data, type Doc } from "@/data";
import { Code, GitHubLink, Markdown, SideIndex, WithSide, tocOf, type IndexGroup } from "@/parts";
import { href, route } from "@/route";
import { blob } from "@/repo";

const EXAMPLES = "examples";
const exampleId = (name: string): string => name.replace(/\.moph$/, "");

// #/docs/<鍵>/<見出し>
const currentKey = (): string => route().sub.split("/")[0] || data.docs[0].key;
const anchor = (): string => route().sub.split("/").slice(1).join("/");
const currentDoc = (): Doc | undefined => data.docs.find((d) => d.key === currentKey());

const groups = (): IndexGroup[] => {
  const key = currentKey();
  const list: IndexGroup = {
    items: [
      ...data.docs.map((d) => ({ label: d.title, href: href("docs", d.key), active: d.key === key })),
      { label: "構文の例", href: href("docs", EXAMPLES), active: key === EXAMPLES },
    ],
  };
  const doc = currentDoc();
  const toc: IndexGroup[] = doc && doc.path.endsWith(".md")
    ? [{ title: "見出し", items: tocOf(doc.text).map((h) => ({ label: h.text, href: href("docs", `${doc.key}/${h.id}`), depth: h.depth, active: anchor() === h.id })) }]
    : key === EXAMPLES
      ? [{ title: "例", items: data.examples.map((e) => ({ label: exampleId(e.name), href: href("docs", `${EXAMPLES}/${exampleId(e.name)}`), active: anchor() === exampleId(e.name) })) }]
      : [];
  return [list, ...toc];
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
      <Text size="sm" tone="muted">構文ごとの短いスクリプト。</Text>
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
  <WithSide side={<SideIndex groups={groups()} />}>
    <Show when={currentKey() === EXAMPLES} fallback={<Show when={currentDoc()} fallback={<Text tone="muted">文書がありません</Text>}>{(d) => <DocBody doc={d()} />}</Show>}>
      <Examples />
    </Show>
  </WithSide>
);
