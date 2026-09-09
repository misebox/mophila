import { Show, type Component } from "solid-js";
import { Heading, Stack, Text } from "@/components/ui";
import { data } from "@/data";
import { Code, GitHubLink, Lead } from "@/parts";
import { blob, repo, repoName } from "@/repo";

const INSTALL = `git clone ${repo}.git
cd ${repoName}
cargo install --path .`;

const COMMANDS = [
  ["mophila preview first.moph", "ウィンドウで実時間再生する。Space で一時停止、← → で移動"],
  ["mophila render first.moph -o first.mp4 --size fhd --fps 30", "動画にする。--size は 幅x高さ か 720p / fhd / 4k など"],
  ["mophila render first.moph -o first.png --at 2s", "指定した時刻の 1 枚を画像にする"],
  ["mophila timeline first.moph", "何が、いつ、どう変わるかをテキストで出す"],
];

export const Start: Component = () => {
  const first = data.samples.find((s) => s.name === "first");
  return (
    <Stack gap={6} class="narrow">
      <Stack gap={2}>
        <Heading level={1} size="2xl">使い方</Heading>
        <Lead>インストールして、最初の 1 本を動かすまで。</Lead>
      </Stack>

      <Stack gap={2}>
        <Heading level={2} size="lg">必要なもの</Heading>
        <ul class="plain">
          <li>Rust (edition 2024)</li>
          <li>ffmpeg と ffprobe (動画の出力、音声の読み込み)</li>
          <li>GPU (wgpu が使えるもの)</li>
        </ul>
      </Stack>

      <Stack gap={2}>
        <Heading level={2} size="lg">インストール</Heading>
        <Text>リポジトリを clone して cargo でビルドすると、mophila コマンドが入ります。</Text>
        <Code text={INSTALL} />
      </Stack>

      <Show when={first}>
        {(s) => (
          <Stack gap={3}>
            <Heading level={2} size="lg">最初の 1 本</Heading>
            <Text>丸が右へ動き、四角が横に広がり、文字が現れる 4 秒。3 つの図形が、それぞれ別の motion で別の時刻に変わります。</Text>
            <Show when={s().media}>
              <video class="sample" src={`media/${s().name}.mp4`} autoplay muted loop playsinline preload="metadata" />
            </Show>
            <Text>次の内容を first.moph という名前で保存します。</Text>
            <Code text={s().code} />
            <ul class="plain">
              <li>View は 16 x 9 の箱。座標はこの箱の中の値で、ピクセルは出力のときに決める</li>
              <li>place で図形を箱に置く。apos! は基準点 (center や bottomLeft) と座標</li>
              <li>context ... motion で「何秒にどの属性がいくつになるか」を書く。行末の :ease は補間の仕方</li>
              <li>Timeline に motion を置く時刻 (at) をずらすと、それぞれ別のタイミングで動き出す</li>
            </ul>
            <GitHubLink href={blob(`samples/${s().name}.moph`)} />
          </Stack>
        )}
      </Show>

      <Stack gap={2}>
        <Heading level={2} size="lg">動かす</Heading>
        <dl class="commands">
          {COMMANDS.map(([cmd, doc]) => (
            <>
              <dt><code>{cmd}</code></dt>
              <dd><Text size="sm" tone="muted">{doc}</Text></dd>
            </>
          ))}
        </dl>
      </Stack>
    </Stack>
  );
};
