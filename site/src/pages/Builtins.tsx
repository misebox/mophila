import { For, Show, Switch, Match, type Component } from "solid-js";
import { Heading, Stack, Table, Text } from "@/components/ui";
import { data, type Entry, type TypeDoc } from "@/data";
import { Faded, SideIndex, WithSide } from "@/parts";
import { href, route } from "@/route";

const EntryTable: Component<{ rows: Entry[] }> = (props) => (
  <Table
    columns={[
      { key: "signature", header: "書き方", render: (v) => <code>{String(v)}</code> },
      { key: "returns", header: "戻り値", render: (v) => <code>{String(v)}</code> },
      { key: "doc", header: "説明" },
    ]}
    data={props.rows}
    rowKey={(r) => r.signature}
  />
);

const isConstructor = (b: Entry): boolean => b.name.endsWith("!");
const current = (): string => route().sub || "functions";
const typeOf = (name: string): TypeDoc | undefined => data.types.find((t) => t.name === name);
// 分類は言語が定義している union (Shape / Paint) だけ。属さない型はまとめて「型」
const unions = (): string[] => [...new Set(data.types.map((t) => t.union).filter((u) => u !== ""))];
const typeLinks = (match: (t: TypeDoc) => boolean) =>
  data.types.filter(match).map((t) => ({ label: t.name, href: href("builtins", t.name), active: current() === t.name }));

const currentLabel = (): string => (current() === "functions" ? "Function" : current() === "constructors" ? "Record" : current());

const Functions: Component = () => (
  <Stack gap={3}>
    <Heading level={1} size="xl">Function</Heading>
    <Text tone="muted">import なしで呼べる関数。</Text>
    <EntryTable rows={data.builtins.filter((b) => !isConstructor(b))} />
  </Stack>
);

const Constructors: Component = () => (
  <Stack gap={3}>
    <Heading level={1} size="xl">Record</Heading>
    <Text tone="muted">名前と、型の付いたフィールドを持つ値。<code>name!(...)</code> で作り、フィールドは <code>v.x</code> のように名前で読む。同じ名前で引数の違う定義を複数持てる。型の決まった場所には素の Tuple も書ける (<a href={href("docs", "spec/3-4-record")}>仕様 3.4</a>)。自分で作るときは <code>record name(field: Type, ...)</code>。</Text>
    <EntryTable rows={data.builtins.filter(isConstructor)} />
  </Stack>
);

// 1 つの型: 作り方、属性 (new で渡すもの)、メソッドと属性 (. で読むもの)
const TypePage: Component<{ type: TypeDoc }> = (props) => (
  <Stack gap={3}>
    <Show when={props.type.union}>
      <Text size="sm" tone="muted"><code>{props.type.union}</code> の 1 つ</Text>
    </Show>
    <Heading level={1} size="xl">{props.type.name}</Heading>
    <Text>{props.type.doc}</Text>
    <pre class="code"><code>{props.type.make}</code></pre>
    <Show when={props.type.attrs.length > 0}>
      <Stack gap={1}>
        <Heading level={2} size="sm" class="sub">属性</Heading>
        <Table
          columns={[
            { key: "name", header: "名前", render: (v) => <code>{String(v)}</code> },
            { key: "type", header: "型" },
            { key: "doc", header: "説明" },
          ]}
          data={props.type.attrs}
          rowKey={(r) => r.name}
        />
      </Stack>
    </Show>
    <Show when={props.type.methods.length > 0}>
      <Stack gap={1}>
        <Heading level={2} size="sm" class="sub">メソッド</Heading>
        <Table
          columns={[
            { key: "signature", header: "書き方", render: (v) => <code>{String(v)}</code> },
            { key: "returns", header: "戻り値", render: (v) => <code>{String(v)}</code> },
            { key: "doc", header: "説明" },
          ]}
          data={props.type.methods}
          rowKey={(m) => m.signature}
        />
      </Stack>
    </Show>
  </Stack>
);

const Content: Component<{ id: string }> = (props) => (
  <Switch fallback={<Functions />}>
    <Match when={props.id === "constructors"}><Constructors /></Match>
    <Match when={typeOf(props.id)}>{(t) => <TypePage type={t()} />}</Match>
  </Switch>
);

export const Builtins: Component = () => (
  <WithSide
    current={currentLabel()}
    side={
      <SideIndex
        groups={[
          {
            items: [
              { label: "Function", href: href("builtins", "functions"), active: current() === "functions" },
              { label: "Record", href: href("builtins", "constructors"), active: current() === "constructors" },
            ],
          },
          { title: "Type", items: typeLinks((t) => t.union === "") },
          ...unions().map((u) => ({ title: u, items: typeLinks((t) => t.union === u) })),
        ]}
      />
    }
  >
    <Faded key={current()}>{(id) => <Content id={id} />}</Faded>
  </WithSide>
);
