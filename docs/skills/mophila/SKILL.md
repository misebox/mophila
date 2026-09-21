---
name: mophila
description: Write or debug mophila (.moph) motion graphics scripts. Entry point: language.md for the language, presentation.md for explanatory videos, design.md for how it looks.
---

# mophila を書く

図形とその動きを書いて、動画や画像にする言語。インタプリタが毎回ソースを解釈する。

| 読むもの | 中身 |
|---|---|
| [language.md](language.md) | 言語の使い方。他の言語と違うところ、型の作り方、時間の書き方、組み合わせて再利用する |
| [presentation.md](presentation.md) | 解説する動画の作り方。場面の組み立て、読める時間、確認の手順 |
| [design.md](design.md) | 見た目。色、構図、文字、動きの質 |

**名前はここに書かない。** 型・属性・メソッド・module の関数と引数は変わるので、
この 4 つの文書は「どう考えて、どう書くか」だけを書く。名前と引数は毎回引く。

- `mophila doc` — いま使っている実行ファイルの、型・属性・メソッド・エラー・math (JSON)
- 下のリファレンス
- エディタを繋いでいれば補完と hover。`(` の中では必須の引数が先に出る

図形を自分で並べる前に、標準 module に部品が無いか見る。

迷ったら、公開されているものを見る。仕様は変わるので、こちらが正。

| | |
|---|---|
| <https://misebox.github.io/mophila/#/docs/spec> | 言語仕様 (意味と規則) |
| <https://misebox.github.io/mophila/#/builtins> | 型・属性・メソッド・エラーの一覧 |
| <https://misebox.github.io/mophila/#/lib> | 標準 module の関数と引数 |
| <https://misebox.github.io/mophila/#/samples> | 動く例 |
| <https://misebox.github.io/mophila/#/docs/examples> | 構文ごとの短い見本 |
| <https://misebox.github.io/mophila/llms-full.txt> | 上の全部を 1 ファイルにしたもの |
| <https://raw.githubusercontent.com/misebox/mophila/main/docs/mophila.gbnf> | 形式文法 (GBNF) |

## 最小の形

```
let v = View(box = Vector(16, 9))          # 座標系。ピクセルは持たない
let c = Circle(position = Pos(8, 4.5), radius = 1, fill = #e04040)
v.place(c)
v.addTrack(motion (t) {
  0s: c.radius = 1
  2s: c.radius = 2 :ease
})
output v
```

## コマンド

```
mophila run a.moph                             # 描画せず実行。log と型エラーの確認
mophila run                                    # mophila.yaml の entry を実行
mophila render a.moph -o out.png --at 2.5s     # 1 フレーム。形式は拡張子で決まる
mophila render a.moph -o out.mp4 --size 360p   # 動画 (fps 既定 10)
mophila render a.moph -o out.mp4 --jobs 4      # 組み立てを 4 スレッドに分ける (重い絵だけ速くなる)
mophila render a.moph -o out.mp4 --trim 00:15..00:30   # 区間だけ
mophila preview a.moph [--loop] [--at 1s]      # ウィンドウで再生
mophila timeline a.moph [--filter k=v]         # 何がいつどう変わるかの一覧
mophila sheet a.moph -o sheet.png --times 3s,12s   # 場面の格子画像
mophila subs a.moph -o subs.srt                # 字幕だけをファイルに (.vtt も)
mophila fonts                                  # 使えるフォント名 (font に書ける名前)
mophila doc                                    # 型・属性・メソッド・エラーの一覧 (JSON)
mophila --version                              # 版
```

全部の引数は `mophila <コマンド> --help`。

preview は Space で一時停止、← → / h l で 10 秒 (停止中は 1 秒)、Shift で 10% 移動、s でステータスバーの出し入れ、q で終了。最後まで行くと最後の場面で止まる。
時間・割合・fps・一時停止は絵の下端のステータスバーに出る。
コマは少し先まで描いておく。中身の時刻は実時間に貼り付くので音とずれない。重い場面では出す間隔が広がり、fps が落ちる。

書いたら必ず `run` を通す。動きを確かめるときは小さいサイズと短い尺で。
