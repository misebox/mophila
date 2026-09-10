import { For, Show, Switch, Match, type Component, type JSX } from "solid-js";
import { Heading, Stack, Table, Text } from "@/components/ui";
import { data, type Entry, type TypeDoc } from "@/data";
import { Faded, SideIndex, TypeText, WithSide } from "@/parts";
import { href, route } from "@/route";

const current = (): string => route().sub || "functions";
const typeOf = (name: string): TypeDoc | undefined => data.types.find((t) => t.name === name);
const currentLabel = (): string => (current() === "functions" ? "Function" : current());

const EntryTable: Component<{ rows: Entry[] }> = (props) => (
  <Table
    columns={[
      { key: "signature", header: "書き方", render: (v) => <code>{String(v)}</code> },
      { key: "returns", header: "戻り値", render: (v) => <code><TypeText text={String(v)} /></code> },
      { key: "doc", header: "説明" },
    ]}
    data={props.rows}
    rowKey={(r) => r.signature}
  />
);

const Functions: Component = () => (
  <Stack gap={3}>
    <Heading level={1} size="xl">Function</Heading>
    <Text tone="muted">import なしで呼べる関数。</Text>
    <EntryTable rows={data.builtins} />
  </Stack>
);

const Part: Component<{ title: string; note?: string; children: JSX.Element }> = (props) => (
  <Stack gap={1}>
    <Heading level={2} size="sm" class="sub">{props.title}</Heading>
    {props.children}
    <Show when={props.note}>{(n) => <Text size="sm" tone="muted">{n()}</Text>}</Show>
  </Stack>
);

const TypePage: Component<{ type: TypeDoc }> = (props) => (
  <Stack gap={3}>
    <Show when={props.type.union && props.type.union !== props.type.category}>
      <Text size="sm" tone="muted"><code>{props.type.union}</code> の 1 つ</Text>
    </Show>
    <Heading level={1} size="xl">{props.type.name}</Heading>
    <Text>{props.type.doc}</Text>
    <Show when={props.type.members.length > 0}>
      <Part title="まとめている型">
        <Text><TypeText text={props.type.members.join(" | ")} /></Text>
      </Part>
    </Show>
    <Show when={props.type.make}>
      <Part title="作り方" note="ここに並んでいるものが、この型を作る書き方の全部。">
        <pre class="code"><code>{props.type.make}</code></pre>
      </Part>
    </Show>
    <Show when={props.type.values.length > 0}>
      <Part title="値" note="この型が取れる値は、ここに並んでいるものだけ。">
        <Table
          columns={[
            { key: "value", header: "値", render: (v) => <code>{String(v)}</code> },
            { key: "doc", header: "意味" },
          ]}
          data={props.type.values}
          rowKey={(r) => r.value}
        />
      </Part>
    </Show>
    <Show when={props.type.attrs.length > 0}>
      <Part title="属性">
        <Table
          columns={[
            { key: "name", header: "名前", render: (v) => <code>{String(v)}</code> },
            { key: "type", header: "型", render: (v) => <code><TypeText text={String(v)} /></code> },
            { key: "doc", header: "説明" },
          ]}
          data={props.type.attrs}
          rowKey={(r) => r.name}
        />
      </Part>
    </Show>
    <Show when={props.type.methods.length > 0}>
      <Part title="メソッド">
        <div class="methods">
          <For each={props.type.methods}>
            {(m) => (
              <div class="method">
                <div class="method-sig">
                  <code><TypeText text={m.signature} /></code>
                  <Show when={m.returns}>{(r) => <code class="arrow"> -&gt; <TypeText text={r()} /></code>}</Show>
                </div>
                <p class="method-doc">{m.doc}</p>
              </div>
            )}
          </For>
        </div>
      </Part>
    </Show>
  </Stack>
);

const Content: Component<{ id: string }> = (props) => (
  <Switch fallback={<Functions />}>
    <Match when={typeOf(props.id)}>{(t) => <TypePage type={t()} />}</Match>
  </Switch>
);

const typeLinks = (category: string) =>
  data.types.filter((t) => t.category === category).map((t) => ({ label: t.name, href: href("builtins", t.name), active: current() === t.name }));

export const Builtins: Component = () => (
  <WithSide
    current={currentLabel()}
    side={
      <SideIndex
        groups={[
          { items: [{ label: "Function", href: href("builtins", "functions"), active: current() === "functions" }] },
          ...data.categories.map((c) => ({ title: c, items: typeLinks(c) })),
        ]}
      />
    }
  >
    <Faded key={current()}>{(id) => <Content id={id} />}</Faded>
  </WithSide>
);
