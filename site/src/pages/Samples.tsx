import { Show, type Component } from "solid-js";
import { Heading, Stack, Text } from "@/components/ui";
import { data, type Sample } from "@/data";
import { Code, Faded, GitHubLink, SideIndex, WithSide } from "@/parts";
import { href, route } from "@/route";
import { blob } from "@/repo";

const seconds = (n: number): string => (n >= 60 ? `${Math.floor(n / 60)} 分 ${Math.round(n % 60)} 秒` : `${n} 秒`);

// 大きさ、fps、長さ。全体より短い区間なら、その時刻も
const Meta: Component<{ sample: Sample }> = (props) => (
  <Show when={props.sample.media}>
    {(m) => (
      <Text size="sm" tone="muted" class="meta">
        {m().width}×{m().height}、{m().fps} fps、{seconds(m().seconds)}
        <Show when={m().seconds < props.sample.length}> (全体 {seconds(props.sample.length)} のうち {props.sample.trim || `最初の ${seconds(m().seconds)}`})</Show>
      </Text>
    )}
  </Show>
);

export const Samples: Component = () => {
  const listed = (): Sample[] => data.samples.filter((s) => s.listed);
const current = (): Sample => data.samples.find((s) => s.name === route().sub) ?? listed()[0];
  return (
    <WithSide current={current().name + ".moph"} side={<SideIndex groups={[{ items: listed().map((s) => ({ label: s.name, href: href("samples", s.name), active: s.name === current().name })) }]} />}>
      <Faded key={current().name}>
        {(name) => {
          const s = data.samples.find((x) => x.name === name) ?? data.samples[0];
          return (
            <Stack gap={3}>
              <Heading level={1} size="xl">{s.name}.moph</Heading>
              <Text>{s.desc}</Text>
              <Show when={s.media}>
                {(m) => <video class="sample" style={{ "--native": `${m().width}px` }} src={`media/${s.name}.mp4`} autoplay muted loop playsinline preload="metadata" />}
              </Show>
              <Meta sample={s} />
              <Code text={s.code} />
              <GitHubLink href={blob(`examples/gallery/${s.name}.moph`)} />
            </Stack>
          );
        }}
      </Faded>
    </WithSide>
  );
};
