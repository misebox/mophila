import { Show, type Component } from "solid-js";
import { Link } from "@/components/ui";
import { data } from "@/data";
import { Code } from "@/parts";
import { href } from "@/route";
import { blob, repo } from "@/repo";


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
        <Show when={data.samples.find((s) => s.name === "circle")}>{(s) => <Code text={s().code} />}</Show>
      </section>
      <section>
        <h2>特徴</h2>
        <ul class="plain">
          <li><strong>変化は、時刻と値の表で書く。</strong>間は補間されます。表は使い回せて、<code>fit</code> で長さを変え、<code>reverse</code> で逆再生にできます</li>
          <li><strong>絵は、時刻の関数。</strong>どのフレームもその時刻だけから決まるので、途中へ飛んでも、1 枚だけ書き出しても同じ絵になります</li>
          <li><strong>塗りを、関数で書ける。</strong>位置と時刻から色を返す関数を Shader に渡すと、GPU が全ピクセルで走らせます。フラクタルもこれで描いています</li>
          <li><strong>作った動画が、部品になる。</strong>ファイルを import すれば、別の動画の中にそのまま置けます</li>
          <li><strong>音も字幕も、同じ Timeline。</strong>時刻を指定して置くと、render が動画の音声トラックと字幕トラックにします</li>
        </ul>
      </section>
      <section>
        <h2>LLM を使って書く</h2>
        <p>
          言語は小さく、文法は <Link href={blob("docs/mophila.gbnf")} external>GBNF</Link> に、書き方の要点は <Link href={blob("docs/skills/mophila/SKILL.md")} external>skill</Link> にまとめてあります。
          これと Language Server を渡せば、Claude Code などにスクリプトを書いてもらい、そのまま render できます。
        </p>
      </section>
    </div>
  </div>
);
