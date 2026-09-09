import { Show, type Component } from "solid-js";
import { data } from "@/data";
import { Code, GitHubLink } from "@/parts";
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
    <div class="reading">
      <section>
        <h1>使い方</h1>
        <p class="lead">インストールして、最初の 1 本を動かすまで。</p>
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

      <Show when={first}>
        {(s) => (
          <section>
            <h2>最初の 1 本</h2>
            <p>丸が右へ動き、四角が横に広がり、文字が現れる 4 秒。3 つの図形が、それぞれ別の motion で別の時刻に変わります。</p>
            <Show when={s().media}>
              <video class="sample" src={`media/${s().name}.mp4`} autoplay muted loop playsinline preload="metadata" />
            </Show>
            <p>次の内容を first.moph という名前で保存します。</p>
            <Code text={s().code} />
            <ul class="plain">
              <li>View は 16 x 9 の箱。座標はこの箱の中の値で、ピクセルは出力のときに決める</li>
              <li>place で図形を箱に置く。apos! は基準点 (center や bottomLeft) と座標</li>
              <li>context ... motion で「何秒にどの属性がいくつになるか」を書く。行末の :ease は補間の仕方</li>
              <li>Timeline に motion を置く時刻 (at) をずらすと、それぞれ別のタイミングで動き出す</li>
            </ul>
            <GitHubLink href={blob(`samples/${s().name}.moph`)} />
          </section>
        )}
      </Show>

      <section>
        <h2>動かす</h2>
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
