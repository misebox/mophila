import { Show, type Component } from "solid-js";
import { Heading, Stack, Text } from "@/components/ui";
import { data, type Sample } from "@/data";
import { Code, GitHubLink, SideIndex, WithSide } from "@/parts";
import { href, route } from "@/route";
import { blob } from "@/repo";

const seconds = (n: number): string => (n >= 60 ? `${Math.floor(n / 60)} 分 ${Math.round(n % 60)} 秒` : `${n} 秒`);

// 大きさ、fps、長さ。全体より短い区間なら、その時刻も
const Meta: Component<{ sample: Sample }> = (props) => (
  <Show when={props.sample.media}>
    {(m) => (
      <Text size="sm" tone="muted" class="meta">
        {m().width} x {m().height} · {m().fps} fps · {seconds(m().seconds)}
        <Show when={m().seconds < props.sample.length}> (全体 {seconds(props.sample.length)} のうち {props.sample.trim || `最初の ${seconds(m().seconds)}`})</Show>
      </Text>
    )}
  </Show>
);

export const Samples: Component = () => {
  const current = (): Sample => data.samples.find((s) => s.name === route().sub) ?? data.samples[0];
  return (
    <WithSide side={<SideIndex groups={[{ items: data.samples.map((s) => ({ label: s.name, href: href("samples", s.name), active: s.name === current().name })) }]} />}>
      <Stack gap={3}>
        <Heading level={1} size="xl">{current().name}.moph</Heading>
        <Text>{current().desc}</Text>
        <Show when={current().media}>
          {(m) => <video class="sample" style={{ "--native": `${m().width}px` }} src={`media/${current().name}.mp4`} autoplay muted loop playsinline preload="metadata" />}
        </Show>
        <Meta sample={current()} />
        <Code text={current().code} />
        <GitHubLink href={blob(`samples/${current().name}.moph`)} />
      </Stack>
    </WithSide>
  );
};
