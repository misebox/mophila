import { Show, type Component } from "solid-js";
import { data } from "@/data";
import { Code, GitHubLink, Markdown } from "@/parts";
import { blob, repo, repoName } from "@/repo";

const INSTALL = `git clone ${repo}.git
cd ${repoName}
cargo install --path .`;

const COMMANDS = [
  ["mophila preview first.moph", "ウィンドウを開いて実時間で再生する。Space で一時停止、← → で移動"],
  ["mophila render first.moph -o first.mp4 --size fhd --fps 30", "動画にする。--size は 幅x高さ か 720p / fhd / 4k など"],
  ["mophila render first.moph -o first.png --at 2s", "その時刻の 1 フレームを画像にする"],
  ["mophila timeline first.moph", "何がいつどう変わるかを、時刻順の一覧で表示する"],
];

export const Start: Component = () => {
  const circle = data.samples.find((s) => s.name === "circle");
  const first = data.samples.find((s) => s.name === "first");
  const editors = data.docs.find((d) => d.key === "editors");
  return (
    <div class="reading">
      <section>
        <h1>使い方</h1>
        <p class="lead">mophila のインストールから、スクリプトを書いて動画にするまでの手順です。</p>
      </section>

      <section>
        <h2>必要なもの</h2>
        <ul class="plain">
          <li>Rust (edition 2024)</li>
          <li>ffmpeg と ffprobe。動画の書き出しと音声の読み込みに使います</li>
          <li>GPU (wgpu が動くもの)</li>
        </ul>
        <details class="fold">
          <summary>ffmpeg のインストール</summary>
          <p>ffprobe は ffmpeg に付いてきます。入っているかは <code>ffmpeg -version</code> で分かります。</p>
          <dl class="commands">
            <div>
              <dt>brew install ffmpeg</dt>
              <dd>macOS (Homebrew)</dd>
            </div>
            <div>
              <dt>sudo apt install ffmpeg</dt>
              <dd>Debian / Ubuntu</dd>
            </div>
            <div>
              <dt>winget install Gyan.FFmpeg</dt>
              <dd>Windows</dd>
            </div>
          </dl>
        </details>
      </section>

      <section>
        <h2>インストール</h2>
        <p>リポジトリを clone して cargo でビルドすると、mophila コマンドが入ります。</p>
        <Code text={INSTALL} />
      </section>

      <Show when={circle}>
        {(s) => (
          <section>
            <h2>丸を 1 つ置いて、大きくする</h2>
            <p>1 秒後から 3 秒かけて、半径を 1 から 3 にします。時刻と値を表に書くだけで、間は補間されます。</p>
            <Show when={s().media}>
              <video class="sample" src={`media/${s().name}.mp4`} autoplay muted loop playsinline preload="metadata" />
            </Show>
            <p>次の内容を circle.moph という名前で保存します。</p>
            <Code text={s().code} />
            <ul class="plain">
              <li>View は 16 x 9 の箱。座標はこの箱の中の値で、ピクセルは出力のときに決める</li>
              <li>place で図形を箱に置く。Pos は座標と、その座標が図形のどこを指すか (anchor)</li>
              <li>context ... motion で「何秒にどの属性がいくつになるか」を書く。行末の :ease は値の変わり方</li>
              <li>Timeline に置いた時刻 (at) から、その動きが始まる</li>
            </ul>
            <GitHubLink href={blob(`examples/gallery/${s().name}.moph`)} />
          </section>
        )}
      </Show>

      <Show when={first}>
        {(s) => (
          <section>
            <h2>複数のオブジェクトを動かす</h2>
            <p>丸が右へ動き、四角が横に広がり、文字が現れて、最後に消えます。4 つの motion が、それぞれ別の時刻に動きます。</p>
            <Show when={s().media}>
              <video class="sample" src={`media/${s().name}.mp4`} autoplay muted loop playsinline preload="metadata" />
            </Show>
            <p>次の内容を first.moph という名前で保存します。</p>
            <Code text={s().code} />
            <ul class="plain">
              <li>Timeline に motion を置く時刻 (at) をずらすと、それぞれ別のタイミングで動き出す</li>
              <li>動かす 3 つは 1 つの View にまとめてある。View の opacity は中身をまとめて 1 枚として掛かる</li>
              <li>fade_out はその View を薄くする Timeline を返す。図形を 1 つずつ消さなくてよい</li>
            </ul>
            <GitHubLink href={blob(`examples/gallery/${s().name}.moph`)} />
          </section>
        )}
      </Show>

      <Show when={editors}>
        {(d) => (
          <section>
            <h2>エディタ</h2>
            <p>シンタックスハイライトと、補完・ホバー・定義へ移動が使えます。</p>
            <details class="fold">
              <summary>VS Code と Neovim の設定</summary>
              <Markdown text={d().text} />
            </details>
            <GitHubLink href={blob(d().path)} />
          </section>
        )}
      </Show>

      <section>
        <h2>コマンド</h2>
        <dl class="commands">
          {COMMANDS.map(([cmd, doc]) => (
            <div>
              <dt>{cmd}</dt>
              <dd>{doc}</dd>
            </div>
          ))}
        </dl>
      </section>
    </div>
  );
};
