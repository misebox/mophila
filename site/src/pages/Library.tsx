import { For, Show, type Component } from "solid-js";
import { Heading, Stack, Table, Text } from "@/components/ui";
import { data, type Lib, type LibItem } from "@/data";
import { Faded, GitHubLink, SideIndex, WithSide, groupBy, type IndexGroup } from "@/parts";
import { href, route } from "@/route";
import { blob } from "@/repo";

const itemId = (lib: Lib, name: string): string => `${lib.name}.${name}`;
const namesOf = (lib: Lib): string[] => [...lib.entries.map((e) => e.name), ...lib.items.map((it) => it.name)];
const current = (): string => route().sub || data.libs[0].name;

// 目次: module ごとに export。module 名を選ぶとその一覧
const groups = (): IndexGroup[] =>
  data.libs.map((lib) => ({
    title: lib.name,
    href: href("lib", lib.name),
    active: current() === lib.name,
    items: lib.entries.length > 0 ? [] : lib.items.map((it) => ({ label: it.name, href: href("lib", itemId(lib, it.name)), active: current() === itemId(lib, it.name) })),
  }));

const ImportLine: Component<{ lib: Lib; names?: string[] }> = (props) => (
  <pre class="code"><code>{props.lib.entries.length > 0 ? `import ${props.lib.name}` : `import { ${(props.names ?? namesOf(props.lib)).join(", ")} } from ${props.lib.name}`}</code></pre>
);

// module の一覧: Rust のものは表、.moph のものは分類ごとの export の一覧
const ModulePage: Component<{ lib: Lib }> = (props) => (
  <Stack gap={3}>
    <Text size="sm" tone="muted">標準ライブラリ</Text>
    <Heading level={1} size="xl">{props.lib.name}</Heading>
    <ImportLine lib={props.lib} />
    <GitHubLink href={blob(props.lib.path)} />
    <Show when={props.lib.entries.length > 0}>
      <Table
        columns={[
          { key: "signature", header: "書き方", render: (v) => <code>{String(v)}</code> },
          { key: "doc", header: "説明" },
        ]}
        data={props.lib.entries}
        rowKey={(e) => e.signature}
      />
    </Show>
    <For each={groupBy(props.lib.items, (it) => it.category)}>
      {([category, items]) => (
        <Stack gap={1}>
          <Heading level={2} size="sm" class="sub">{category}</Heading>
          <Table
            columns={[
              { key: "name", header: "名前", render: (v, it) => <a href={href("lib", itemId(props.lib, it.name))}><code>{String(v)}</code></a> },
              { key: "returns", header: "型", render: (v) => <code>{(v as { type: string }).type}</code> },
              { key: "summary", header: "説明" },
            ]}
            data={items}
            rowKey={(it) => it.name}
          />
        </Stack>
      )}
    </For>
  </Stack>
);

const ItemPage: Component<{ lib: Lib; item: LibItem }> = (props) => (
  <Stack gap={3}>
    <Text size="sm" tone="muted"><a href={href("lib", props.lib.name)}>{props.lib.name}</a> / {props.item.category}</Text>
    <Heading level={1} size="xl">{props.item.name}</Heading>
    <Text>{props.item.summary}</Text>
    <pre class="code"><code>{`import { ${props.item.name} } from ${props.lib.name}\n\n`}{props.item.call}{props.item.returns.type ? (props.item.isFunc ? ` -> ${props.item.returns.type}` : `: ${props.item.returns.type}`) : ""}</code></pre>
    <Show when={props.item.returns.type || props.item.returns.doc}>
      <Stack gap={1}>
        <Heading level={2} size="sm" class="sub">{props.item.isFunc ? "戻り値" : "型"}</Heading>
        <Text><code>{props.item.returns.type}</code>{props.item.returns.doc ? ` — ${props.item.returns.doc}` : ""}</Text>
      </Stack>
    </Show>
    <Show when={props.item.params.length > 0}>
      <Stack gap={1}>
        <Heading level={2} size="sm" class="sub">引数</Heading>
        <Table
          columns={[
            { key: "name", header: "名前", render: (v) => <code>{String(v)}</code> },
            { key: "type", header: "型", render: (v) => <code>{String(v)}</code> },
            { key: "default", header: "既定", render: (v) => (v ? <code>{String(v)}</code> : <span class="none">なし</span>) },
            { key: "doc", header: "説明" },
          ]}
          data={props.item.params}
          rowKey={(r) => r.name}
        />
      </Stack>
    </Show>
    <GitHubLink href={blob(props.lib.path)} />
  </Stack>
);

const Content: Component<{ id: string }> = (props) => {
  const lib = (): Lib | undefined => data.libs.find((l) => l.name === props.id || props.id.startsWith(`${l.name}.`));
  const item = (): LibItem | undefined => lib()?.items.find((it) => itemId(lib() as Lib, it.name) === props.id);
  return (
    <Show when={lib()} fallback={<Text tone="muted">ありません</Text>}>
      {(l) => (
        <Show when={item()} fallback={<ModulePage lib={l()} />}>
          {(it) => <ItemPage lib={l()} item={it()} />}
        </Show>
      )}
    </Show>
  );
};

export const Library: Component = () => (
  <WithSide current={current()} side={<SideIndex groups={groups()} />}>
    <Faded key={current()}>{(id) => <Content id={id} />}</Faded>
  </WithSide>
);
