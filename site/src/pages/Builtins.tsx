import { For, Show, type Component } from "solid-js";
import { Heading, Stack, Table, Text } from "@/components/ui";
import { data, type Entry, type Method, type TypeDoc } from "@/data";
import { Section, SideIndex, WithSide, type IndexItem } from "@/parts";
import { href, route } from "@/route";

const EntryTable: Component<{ rows: Entry[] }> = (props) => (
  <Table
    columns={[
      { key: "signature", header: "書き方", render: (v) => <code>{String(v)}</code> },
      { key: "doc", header: "説明" },
    ]}
    data={props.rows}
    rowKey={(r) => r.signature}
  />
);

const isConstructor = (b: Entry): boolean => b.name.endsWith("!");
const typeId = (name: string): string => `type-${name}`;
const receiverId = (name: string): string => `method-${name.replace(/[^A-Za-z]+/g, "-")}`;

const TypeBlock: Component<{ type: TypeDoc }> = (props) => (
  <Stack gap={2} id={typeId(props.type.name)} class="entry">
    <Heading level={3} size="md"><code>{props.type.name}</code></Heading>
    <Text size="sm" tone="muted">{props.type.doc}</Text>
    <Show when={props.type.attrs.length > 0}>
      <Table
        columns={[
          { key: "name", header: "属性", render: (v) => <code>{String(v)}</code> },
          { key: "type", header: "型" },
          { key: "doc", header: "説明" },
        ]}
        data={props.type.attrs}
        rowKey={(r) => r.name}
      />
    </Show>
  </Stack>
);

const link = (label: string, id: string): IndexItem => ({ label, href: href("builtins", id), active: route().sub === id });

const receivers = (): string[] => [...new Set(data.methods.map((m) => m.receiver))];

export const Builtins: Component = () => (
  <WithSide
    side={
      <SideIndex
        groups={[
          { items: [link("関数", "functions"), link("コンストラクタ", "constructors")] },
          { title: "型", items: data.types.map((t) => link(t.name, typeId(t.name))) },
          { title: "メソッドと属性", items: receivers().map((r) => link(r, receiverId(r))) },
        ]}
      />
    }
  >
    <Stack gap={6}>
      <Section id="functions" title="関数">
        <Text size="sm" tone="muted">import なしで使えるもの。</Text>
        <EntryTable rows={data.builtins.filter((b) => !isConstructor(b))} />
      </Section>
      <Section id="constructors" title="コンストラクタ">
        <Text size="sm" tone="muted">関数ではなく specific tuple (名前と要素の型を持つ Tuple) を作る書き方。型の決まった場所では素の Tuple からも変換される (<a href={href("docs", "spec/3-4-specific-tuple")}>仕様 3.4</a>)。</Text>
        <EntryTable rows={data.builtins.filter(isConstructor)} />
      </Section>
      <Section id="types" title="型と属性">
        <For each={data.types}>{(t) => <TypeBlock type={t} />}</For>
      </Section>
      <Section id="methods" title="メソッドと属性">
        <Text size="sm" tone="muted">値に対して <code>.</code> で呼ぶもの。Motion は <code>motion (t) {"{ ... }"}</code> の式が返す値で、motion 自体は構文。</Text>
        <For each={receivers()}>
          {(r) => (
            <Stack gap={2} id={receiverId(r)} class="entry">
              <Heading level={3} size="md"><code>{r}</code></Heading>
              <Table
                columns={[
                  { key: "signature", header: "書き方", render: (v) => <code>{String(v)}</code> },
                  { key: "doc", header: "説明" },
                ]}
                data={data.methods.filter((m) => m.receiver === r)}
                rowKey={(m: Method) => m.signature}
              />
            </Stack>
          )}
        </For>
      </Section>
    </Stack>
  </WithSide>
);
