import { For, Show, type Component } from "solid-js";
import { Heading, Stack, Table, Text } from "@/components/ui";
import { data, type Lib, type LibItem, type LibSection } from "@/data";
import { GitHubLink, SideIndex, WithSide, type IndexGroup } from "@/parts";
import { href, route } from "@/route";
import { blob } from "@/repo";

const moduleName = (lib: Lib): string => lib.file.replace(/\.moph$/, "");
const itemId = (lib: Lib, item: LibItem): string => `${moduleName(lib)}.${item.name}`;

const Item: Component<{ lib: Lib; item: LibItem }> = (props) => (
  <Stack gap={2} id={itemId(props.lib, props.item)} class="entry">
    <Heading level={4} size="md"><code>{props.item.signature}</code></Heading>
    <Text>{props.item.summary}</Text>
    <Show when={props.item.params.length > 0 || props.item.returns}>
      <Table
        columns={[
          { key: "name", header: "引数", render: (v) => <code>{String(v)}</code> },
          { key: "doc", header: "説明" },
        ]}
        data={[...props.item.params.map(([name, doc]) => ({ name, doc })), ...(props.item.returns ? [{ name: "戻り値", doc: props.item.returns }] : [])]}
        rowKey={(r) => r.name}
      />
    </Show>
  </Stack>
);

// 数学関数は本体に入っているが、import して使うのでここに置く
const MathSection: Component = () => (
  <section class="section" id="math">
    <Stack gap={1}>
      <Heading level={2} size="lg">math</Heading>
      <pre class="code"><code>import math</code></pre>
    </Stack>
    <Table
      columns={[
        { key: "signature", header: "書き方", render: (v) => <code>{String(v)}</code> },
        { key: "doc", header: "説明" },
      ]}
      data={data.math}
      rowKey={(e) => e.signature}
    />
  </section>
);

const groups = (): IndexGroup[] => [
  { items: [{ label: "math", href: href("lib", "math"), active: route().sub === "math" }] },
  ...data.libs.map((lib) => ({ title: moduleName(lib), items: lib.sections.flatMap((sec) => sec.items.map((it) => ({ label: it.name, href: href("lib", itemId(lib, it)), active: route().sub === itemId(lib, it) }))) })),
];

const exportsOf = (lib: Lib): string[] => lib.sections.flatMap((sec) => sec.items.map((it) => it.name));

const Section: Component<{ lib: Lib; section: LibSection }> = (props) => (
  <Stack gap={2}>
    <Show when={props.section.title}><Heading level={3} size="sm" class="sub">{props.section.title}</Heading></Show>
    <For each={props.section.items}>{(it) => <Item lib={props.lib} item={it} />}</For>
  </Stack>
);

export const Library: Component = () => (
  <WithSide side={<SideIndex groups={groups()} />}>
    <Stack gap={6}>
      <Text size="sm" tone="muted">import して使うもの。基本的なものから順に並べている。同梱の .moph は相対パスで import し、説明はソースのドキュメントコメント (##) から作っている。</Text>
      <MathSection />
      <For each={data.libs}>
        {(lib) => (
          <section class="section" id={moduleName(lib)}>
            <Stack gap={1}>
              <Heading level={2} size="lg">{moduleName(lib)}</Heading>
              <pre class="code"><code>{`import { ${exportsOf(lib).join(", ")} } from ..lib.${moduleName(lib)}`}</code></pre>
              <Text size="sm" tone="muted">samples/ から見た相対パス。自分のファイルの場所に合わせて変える。</Text>
              <GitHubLink href={blob(`lib/${lib.file}`)} />
            </Stack>
            <For each={lib.sections}>{(sec) => <Section lib={lib} section={sec} />}</For>
          </section>
        )}
      </For>
    </Stack>
  </WithSide>
);
