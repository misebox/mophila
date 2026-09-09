#!/usr/bin/env python3
"""ドキュメントページの元データ (site/src/data.json) を作る。ページ自体は site/ の SolidJS アプリ。

元にするもの:
- `mophila doc` の JSON (組み込み、math、メソッド、型)
- lib/*.moph の `##` ドキュメントコメント (export の直前の行。1 行目が要約、@param / @returns)
- samples/*.moph (先頭のコメントが説明。--media を付けると site/public/media/<name>.mp4 を render する)
- 言語仕様と editors/ の README (本文をそのまま入れる)

使い方: scripts/docgen.py [--media]   (先に cargo build)
"""
import json, re, subprocess, sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BIN = ROOT / "target/debug/mophila"
SITE = ROOT / "site"
MEDIA = SITE / "public" / "media"
# サンプルの表示順。intro は長いので動画は付けない
SAMPLES = ["first", "passerby", "walker", "crowd", "fractal", "orbit", "bounce", "bars", "clock", "grid", "julia", "burning_ship", "mandelbrot"]


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


SECTION = re.compile(r"^# -{10} (.+) -{10}$")


def library_docs(path: Path) -> list[dict]:
    """`##` のブロックと、その直後の export をまとめる。`# ---------- 名前 ----------` の行で節に分ける"""
    sections, block = [], []
    items = []
    for line in path.read_text().splitlines():
        if m := SECTION.match(line):
            items = []
            sections.append({"title": m.group(1), "items": items})
            continue
        if line.startswith("##"):
            block.append(line[2:].strip())
            continue
        if block and line.startswith("export "):
            head = signature_of(line)
            name = re.match(r"[A-Za-z_][A-Za-z0-9_]*", head).group(0)
            params, returns, summary = [], "", []
            for b in block:
                if b.startswith("@param "):
                    _, pname, *desc = b.split(" ", 2)
                    params.append((pname, desc[0] if desc else ""))
                elif b.startswith("@returns "):
                    returns = b[len("@returns "):]
                else:
                    summary.append(b)
            if not sections:
                sections.append({"title": "", "items": items})
            items.append({"name": name, "signature": head, "summary": " ".join(summary), "params": params, "returns": returns})
        block = []
    return sections


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
    libs = [{"file": p.name, "sections": library_docs(p)} for p in sorted((ROOT / "lib").glob("*.moph"))]
    samples = [sample_info(ROOT / "samples" / f"{n}.moph") for n in SAMPLES]
    if media:
        for s in samples:
            render_media(s["name"], ROOT / "samples" / f"{s['name']}.moph")
    for s in samples:
        mp4 = MEDIA / f"{s['name']}.mp4"
        s["media"] = media_info(mp4) if mp4.exists() else None
        s["trim"] = TRIM.get(s["name"], "")
    docs = [{"path": path, "key": key, "title": title, "text": (ROOT / path).read_text()} for path, key, title in DOCS]
    examples = [{"name": p.name, "code": p.read_text()} for p in sorted((ROOT / "examples").glob("*.moph"))]
    data = {"builtins": d["builtins"], "math": d["math"], "methods": d["methods"], "types": d["types"], "libs": libs, "samples": samples, "docs": docs, "examples": examples}
    out = SITE / "src" / "data.json"
    out.write_text(json.dumps(data, ensure_ascii=False, indent=1))
    have = sum(1 for s in samples if s["media"])
    print(f"{out.relative_to(ROOT)}: {len(samples)} samples, {sum(len(s['items']) for l in libs for s in l['sections'])} library items, media {have}")


if __name__ == "__main__":
    main()
