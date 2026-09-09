import { type Component } from "solid-js";
import { Heading, Link, Stack, Text } from "@/components/ui";
import { Code, Lead } from "@/parts";
import { href } from "@/route";
import { blob, repo } from "@/repo";

const CIRCLE = `let v = new View { box: (16, 9) }
let c = new Circle { position: apos!(:center, 8, 4.5), radius: 1, fill: #ffb454 }
v.place(c)

let grow = context c as o {
  motion (t) {
    0s: o.radius = 1
    3s: o.radius = 3 :ease
  }
}
let track = new Timeline {}
track.place(grow, at: 1s)
v.addTrack(track)
output v`;

export const Overview: Component = () => (
  <Stack gap={6} class="narrow">
    <Stack gap={2}>
      <Heading level={1} size="3xl">mophila</Heading>
      <Lead>動画をコードで書く言語。スクリプトを解釈し、GPU で描いて、ffmpeg で動画か画像にします。</Lead>
    </Stack>
    <figure class="hero">
      <video class="sample" src="media/passerby.mp4" autoplay muted loop playsinline preload="metadata" />
      <figcaption><Text size="sm" tone="muted">木と歩く人を別のファイルから import して置いただけの 8 秒。</Text></figcaption>
    </figure>
    <Stack gap={2}>
      <Text>丸を 1 つ置いて、1 秒後から 3 秒かけて大きくする。</Text>
      <Code text={CIRCLE} />
    </Stack>
    <Stack gap={3}>
      <Heading level={2} size="lg">考え方</Heading>
      <ul class="plain">
        <li>座標は箱 (View) の中の値で書く。ピクセルの大きさは出力のときに決める</li>
        <li>変化は時刻と値の表 (motion) で書く。間は補間され、duration で全体が伸び縮みする</li>
        <li>作った View は部品。別の View に、別の大きさで置ける。ファイルも import で部品になる</li>
        <li>どの時刻の絵も、その時刻だけから決まる。途中へ飛んでも同じ絵になる</li>
        <li>音声と字幕も同じ Timeline に置く。Shader で位置と時刻から色を決める塗りが書ける</li>
      </ul>
    </Stack>
    <Stack gap={2}>
      <Heading level={2} size="lg">LLM に書かせる</Heading>
      <Text>
        言語は小さく、文法は <Link href={blob("docs/mophila.gbnf")} external>GBNF</Link> に、書き方の要点は <Link href={blob("docs/skills/mophila/SKILL.md")} external>skill</Link> にまとめてある。
        これと Language Server を渡せば、Claude Code などにスクリプトを書かせて、そのまま render できる。
      </Text>
    </Stack>
    <div class="links">
      <Link href={href("start")}>使い方</Link>
      <Link href={href("samples")}>サンプル</Link>
      <Link href={repo} external tone="neutral">GitHub</Link>
    </div>
  </Stack>
);
