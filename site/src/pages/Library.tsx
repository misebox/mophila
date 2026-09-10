import { For, Show, type Component, type JSX } from "solid-js";
import { Heading, Stack, Table, Text } from "@/components/ui";
import { data, type Lib, type LibItem } from "@/data";
import { Faded, GitHubLink, SideIndex, TypeText, WithSide, groupBy, type IndexGroup } from "@/parts";
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

// 名前が多いと 1 行に収まらないので、その場合だけ 1 行 1 名前にする
const importLine = (lib: Lib, names: string[]): string => {
  if (lib.entries.length > 0) return `import ${lib.name}`;
  const one = `import { ${names.join(", ")} } from ${lib.name}`;
  return one.length <= 60 ? one : `import {\n${names.map((n) => `  ${n},`).join("\n")}\n} from ${lib.name}`;
};

const ImportLine: Component<{ lib: Lib; names?: string[] }> = (props) => (
  <pre class="code"><code>{importLine(props.lib, props.names ?? namesOf(props.lib))}</code></pre>
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
              { key: "returns", header: "型", render: (v) => <code><TypeText text={(v as { type: string }).type} /></code> },
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

const Part: Component<{ title: string; children: JSX.Element }> = (props) => (
  <Stack gap={1}>
    <Heading level={2} size="sm" class="sub">{props.title}</Heading>
    {props.children}
  </Stack>
);

const signature = (item: LibItem): string =>
  item.returns.type ? `${item.call}${item.isFunc ? ` -> ${item.returns.type}` : `: ${item.returns.type}`}` : item.call;

const ItemPage: Component<{ lib: Lib; item: LibItem }> = (props) => (
  <Stack gap={3}>
    <Text size="sm" tone="muted"><a href={href("lib", props.lib.name)}>{props.lib.name}</a> / {props.item.category}</Text>
    <Heading level={1} size="xl">{props.item.name}</Heading>
    <Text>{props.item.summary}</Text>
    <Part title={props.item.isFunc ? "呼び方" : "型"}>
      <pre class="code"><code><TypeText text={signature(props.item)} /></code></pre>
    </Part>
    <Show when={props.item.params.length > 0}>
      <Part title="引数">
        <Table
          columns={[
            { key: "name", header: "名前", render: (v) => <code>{String(v)}</code> },
            { key: "type", header: "型", render: (v) => <code><TypeText text={String(v)} /></code> },
            { key: "default", header: "既定", render: (v) => (v ? <code>{String(v)}</code> : <span class="none">なし</span>) },
            { key: "doc", header: "説明" },
          ]}
          data={props.item.params}
          rowKey={(r) => r.name}
        />
      </Part>
    </Show>
    <Show when={props.item.returns.type || props.item.returns.doc}>
      <Part title={props.item.isFunc ? "戻り値" : "中身"}>
        <Text><code><TypeText text={props.item.returns.type} /></code>{props.item.returns.doc ? ` — ${props.item.returns.doc}` : ""}</Text>
      </Part>
    </Show>
    <Show when={props.item.example}>
      <Part title="使用例">
        <pre class="code"><code>{props.item.example}</code></pre>
      </Part>
    </Show>
    <Part title="読み込み">
      <pre class="code"><code>{`import { ${props.item.name} } from ${props.lib.name}`}</code></pre>
    </Part>
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
