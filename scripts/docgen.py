#!/usr/bin/env python3
"""ドキュメントページの元データ (site/src/data.json) を作る。ページ自体は site/ の SolidJS アプリ。

元にするもの:
- `mophila doc` の JSON (組み込み、math、メソッド、型)
- src/stdlib/*.moph の `##` ドキュメントコメント (export の直前の行。1 行目が要約、@category / @param 名前 説明 / @returns 説明)。
  引数と戻り値の型は署名から読む。値の export だけ @type {型} で書く
- examples/gallery/*.moph (先頭のコメントが説明。--media を付けると site/public/media/<name>.mp4 を render する)
- 言語仕様と editors/ の README (本文をそのまま入れる)

使い方: scripts/docgen.py [--media]   (先に cargo build)
"""
import json, re, subprocess, sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BIN = ROOT / "target/debug/mophila"
SITE = ROOT / "site"
MEDIA = SITE / "public" / "media"
# サンプルのページに出さないもの (使い方のページで扱う)
ONLY_START = {"circle", "first"}
# サンプルの表示順。intro は長いので動画は付けない
# 並び順。あとのものが前のものを import するように並べる (grid は orbit / bars / clock / bounce を使う)
SAMPLES = ["circle", "first", "shapes", "bounce", "bars", "clock", "orbit", "fractal", "walker", "passerby", "crowd", "grid", "julia", "burning_ship", "mandelbrot"]


def builtin_docs() -> dict:
    return json.loads(subprocess.run([BIN, "doc"], capture_output=True, text=True, check=True).stdout)


def signature_of(line: str) -> str:
    """`export func name(params) {` → `name(params)`、`export let name = ...` → `name`。引数の括弧は入れ子を数えて閉じる"""
    body = re.sub(r"^export (func|let) ", "", line)
    if line.startswith("export let"):
        return body.split("=")[0].strip()
    depth, end = 0, len(body)
    for i, c in enumerate(body):
        if c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
            if depth == 0:
                end = i + 1
                break
    return body[:end]


TAG = re.compile(r"^\{([^}]*)\}\s*")


def param_types(head: str) -> tuple[dict, dict]:
    """署名から、引数の型と既定値を読む"""
    _, args = split_args(head)
    types, defaults = {}, {}
    for a in args:
        left, _, default = a.partition("=")
        pname, _, kind = left.partition(":")
        pname = pname.strip()
        if kind.strip():
            types[pname] = kind.strip()
        if default.strip():
            defaults[pname] = default.strip()
    return types, defaults


def tagged(rest: str, named: bool) -> dict:
    """`{型} 名前 説明` (@param) と `{型} 説明` (@returns) を分ける。型は省略できる"""
    kind = ""
    if m := TAG.match(rest):
        kind, rest = m.group(1), rest[m.end():]
    if named:
        name, _, doc = rest.partition(" ")
        return {"name": name, "type": kind, "doc": doc.strip()}
    return {"type": kind, "doc": rest.strip()}


def split_args(head: str) -> tuple[str, list[str]]:
    """`name(a, b = f(1), c)` を名前と引数の並びに分ける。入れ子の括弧と波括弧は数える"""
    name, _, rest = head.partition("(")
    if not rest:
        return head, []
    body = rest[:-1] if rest.endswith(")") else rest
    args, depth, start = [], 0, 0
    for i, c in enumerate(body):
        if c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
        elif c == "," and depth == 0:
            args.append(body[start:i].strip())
            start = i + 1
    last = body[start:].strip()
    if last:
        args.append(last)
    return name, args


def call_form(head: str) -> str:
    """長い呼び出しは 1 引数 1 行にする"""
    if len(head) <= 60:
        return head
    name, args = split_args(head)
    if not args:
        return head
    lines = ",\n".join(f"  {a}" for a in args)
    return f"{name}(\n{lines},\n)"


def library_docs(path: Path) -> list[dict]:
    """`##` のブロックと、その直後の export をまとめる。category は @category (無ければ空)"""
    items, block = [], []
    for line in path.read_text().splitlines():
        if line.startswith("##"):
            body = line[2:]
            block.append(body[1:] if body.startswith(" ") else body)
            continue
        if block and line.startswith("export "):
            head = signature_of(line)
            name = re.match(r"[A-Za-z_][A-Za-z0-9_]*", head).group(0)
            params, returns, category, summary, example = [], {"type": "", "doc": ""}, "", [], []
            in_example = False
            for raw in block:
                b = raw.strip()
                if b == "@example":
                    in_example = True
                    continue
                if in_example and not b.startswith("@"):
                    example.append(raw)
                    continue
                in_example = False
                if b.startswith("@param "):
                    params.append(tagged(b[len("@param "):], named=True))
                elif b.startswith("@returns ") or b.startswith("@type "):
                    tag = "@returns " if b.startswith("@returns ") else "@type "
                    returns = tagged(b[len(tag):], named=False)
                elif b.startswith("@category "):
                    category = b[len("@category "):]
                else:
                    summary.append(b)
            types, defaults = param_types(head)
            for prm in params:
                prm["default"] = defaults.get(prm["name"], "")
                # 型は署名から。書いていなければドキュメントコメントの {型}
                prm["type"] = types.get(prm["name"], prm["type"])
            if not returns["type"]:
                if m := re.search(r"\)\s*->\s*([^{]+?)\s*\{", line):
                    returns["type"] = m.group(1)
            items.append({"name": name, "category": category, "isFunc": "(" in head, "signature": head, "call": call_form(head), "summary": " ".join(summary), "params": params, "returns": returns, "example": "\n".join(example)})
        block = []
    return items


def script_length(path: Path) -> float:
    """`mophila timeline` の 1 行目 `duration: 3.50s`"""
    first = subprocess.run([BIN, "timeline", str(path)], capture_output=True, text=True, check=True).stdout.splitlines()[0]
    return float(re.search(r"([\d.]+)s", first).group(1))


def sample_info(path: Path) -> dict:
    """説明は先頭のコメント 1 行目 (「name.moph — 説明」の説明の部分)"""
    text = path.read_text()
    first = text.splitlines()[0] if text else ""
    desc = re.sub(r"^\S+\.moph\s*[—–―-]\s*", "", first.lstrip("# ")) if first.startswith("#") else ""
    return {"name": path.stem, "desc": desc, "code": text, "length": script_length(path)}


# ページに載せる区間 (--trim)。無ければ最初の 20 秒
TRIM = {"mandelbrot": "00:48..00:53", "julia": "00:12..00:17", "burning_ship": "00:22..00:27"}
MEDIA_SECONDS = 20
MEDIA_SIZE = "1280x720"
# フラクタルは画面全体が細かく動いてファイルが大きくなるので、画質ではなく大きさを落とす
SMALL = {"mandelbrot": "640x360", "julia": "640x360", "burning_ship": "640x360"}
MEDIA_FPS = 30
MEDIA_CRF = 22         # 線や面の縁が崩れない程度


def render_media(name: str, path: Path) -> None:
    """区間を切って render し、画質を落として小さくする"""
    out = MEDIA / f"{name}.mp4"
    if out.exists():
        return
    MEDIA.mkdir(parents=True, exist_ok=True)
    raw = MEDIA / f"{name}.raw.mp4"
    trim = TRIM.get(name, f"..{MEDIA_SECONDS}s")
    subprocess.run([BIN, "render", str(path), "-o", str(raw), "--size", SMALL.get(name, MEDIA_SIZE), "--fps", str(MEDIA_FPS), "--trim", trim], check=True)
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-i", str(raw), "-an", "-c:v", "libx264", "-crf", str(MEDIA_CRF), "-preset", "slow", "-tune", "animation", "-pix_fmt", "yuv420p", "-movflags", "+faststart", str(out)], check=True)
    raw.unlink()


def media_info(path: Path) -> dict:
    """動画の大きさ、fps、長さを ffprobe で読む"""
    out = subprocess.run(["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height,r_frame_rate:format=duration", "-of", "json", str(path)], capture_output=True, text=True, check=True).stdout
    j = json.loads(out)
    s = j["streams"][0]
    num, den = s["r_frame_rate"].split("/")
    return {"width": s["width"], "height": s["height"], "fps": round(int(num) / int(den), 2), "seconds": round(float(j["format"]["duration"]), 1)}


# サイトに写す文書: (元, URL に使う鍵, 表示名)。skill と GBNF は LLM に渡すものなので、リンクだけ
DOCS = [
    ("docs/mophila-spec.md", "spec", "言語仕様"),
    ("editors/vscode/README.md", "editors", "エディタ (VS Code / Neovim)"),
]


def main() -> None:
    media = "--media" in sys.argv
    d = builtin_docs()
    # 標準ライブラリ。math は本体の表から、.moph は src/stdlib から。基本的なものが先
    libs = [{"name": "math", "path": "src/stdlib/math.rs", "entries": d["math"], "items": []}]
    # .moph の順は本体 (src/stdlib/mod.rs の SCRIPTS) と同じ
    order = re.findall(r'\("(\w+)", include_str!', (ROOT / "src" / "stdlib" / "mod.rs").read_text())
    libs += [{"name": n, "path": f"src/stdlib/{n}.moph", "entries": [], "items": library_docs(ROOT / "src" / "stdlib" / f"{n}.moph")} for n in order]
    samples = [sample_info(ROOT / "examples/gallery" / f"{n}.moph") for n in SAMPLES]
    if media:
        for s in samples:
            render_media(s["name"], ROOT / "examples/gallery" / f"{s['name']}.moph")
    for s in samples:
        mp4 = MEDIA / f"{s['name']}.mp4"
        s["media"] = media_info(mp4) if mp4.exists() else None
        s["trim"] = TRIM.get(s["name"], "")
        s["listed"] = s["name"] not in ONLY_START
    docs = [{"path": path, "key": key, "title": title, "text": (ROOT / path).read_text()} for path, key, title in DOCS]
    examples = [{"name": p.name, "code": p.read_text()} for p in sorted((ROOT / "examples/syntax").glob("*.moph"))]
    data = {"builtins": d["builtins"], "types": d["types"], "categories": d["categories"], "libs": libs, "samples": samples, "docs": docs, "examples": examples}
    out = SITE / "src" / "data.json"
    out.write_text(json.dumps(data, ensure_ascii=False, indent=1))
    have = sum(1 for s in samples if s["media"])
    print(f"{out.relative_to(ROOT)}: {len(samples)} samples, {sum(len(l['items']) for l in libs)} library items, media {have}")


if __name__ == "__main__":
    main()
