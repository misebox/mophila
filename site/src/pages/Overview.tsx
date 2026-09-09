import { type Component } from "solid-js";
import { Link } from "@/components/ui";
import { Code } from "@/parts";
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
  <div class="overview">
    <section class="hero">
      <div>
        <h1 class="hero-title">動画を、<br />コードで書く。</h1>
        <p class="hero-lead">図形と、その動きを書くための言語です。GPU で描画し、動画や画像として書き出します。</p>
        <div class="hero-links">
          <a href={href("start")} class="primary">使い方</a>
          <a href={href("samples")}>サンプル</a>
          <a href={repo} target="_blank" rel="noopener noreferrer">GitHub</a>
        </div>
      </div>
      <figure>
        <video class="sample" src="media/first.mp4" autoplay muted loop playsinline preload="metadata" />
        <figcaption>図形のアニメーションを、短いコードで書けます。</figcaption>
      </figure>
    </section>
    <div class="reading">
      <section>
        <h2>丸を 1 つ置いて、大きくする</h2>
        <p>1 秒後から 3 秒かけて半径を 1 から 3 にする。時刻と値の表を書くだけで、間は補間されます。</p>
        <Code text={CIRCLE} />
      </section>
      <section>
        <h2>考え方</h2>
        <ul class="plain">
          <li>座標は箱 (View) の中の値で書く。ピクセルの大きさは出力のときに決める</li>
          <li>変化は時刻と値の表 (motion) で書く。間は補間され、duration で全体が伸び縮みする</li>
          <li>作った View は部品。別の View に、別の大きさで置ける。ファイルも import で部品になる</li>
          <li>どの時刻の絵も、その時刻だけから決まる。途中へ飛んでも同じ絵になる</li>
          <li>音声と字幕も同じ Timeline に置く。Shader で位置と時刻から色を決める塗りが書ける</li>
        </ul>
      </section>
      <section>
        <h2>LLM に書かせる</h2>
        <p>
          言語は小さく、文法は <Link href={blob("docs/mophila.gbnf")} external>GBNF</Link> に、書き方の要点は <Link href={blob("docs/skills/mophila/SKILL.md")} external>skill</Link> にまとめてあります。
          これと Language Server を渡せば、Claude Code などにスクリプトを書かせて、そのまま render できます。
        </p>
      </section>
    </div>
  </div>
);
