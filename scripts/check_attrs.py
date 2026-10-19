#!/usr/bin/env python3
"""宣言した属性が、本当に絵に効くかを確かめる。

`mophila doc` が挙げる図形と 3D の塗りの属性を 2 通りの値で描いて、絵が変わることを見る。
変わらなければ「書けるのに何も起こらない」属性なので、描画側を直すか宣言から外す。
属性を足したのにここに書き方が無ければ、それも失敗として出る。

`required` の印も見る。必須と書いた属性は、省くと描けないこと。必須と書いていない属性は、
全部省いても描けること。印と描画側がずれたら失敗になる。

使い方: scripts/check_attrs.py   (先に cargo build)
"""
import hashlib
import json
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BIN = ROOT / "target/debug/mophila"

# 図形ごとの、見える形にする属性 (必須でないものも含む)
BASE = {
    "Circle": {"position": "Vector(2, 2)", "radius": "1", "fill": "#e04040"},
    "Ellipse": {"position": "Vector(2, 2)", "rx": "1.5", "ry": "0.5", "fill": "#e04040"},
    "Rect": {"position": "Vector(2, 2)", "w": "2", "h": "1", "fill": "#e04040"},
    "Line": {"from": "Vector(0.5, 0.5)", "to": "Vector(3.5, 3.5)", "stroke": "#e04040", "strokeWidth": "0.3"},
    "Polygon": {"points": "[Vector(0.5, 0.5), Vector(3.5, 1), Vector(2, 3.5)]", "fill": "#e04040"},
    "Path": {"from": "Vector(0.5, 0.5)", "segments": "[(:line, Vector(3.5, 2)), (:line, Vector(1, 3.5))]", "stroke": "#e04040", "strokeWidth": "0.4"},
    "TextArea": {"position": "Vector(2, 2)", "text": '"Ag"', "fontSize": "1.5", "fill": "#e04040"},
}

# 属性ごとの、試す 2 つの値と、効き目を見るために一緒に書いておく属性
TRY = {
    "position": ("Vector(1, 1)", "Vector(3, 3)", {}),
    "anchor": ("Vector(0, 0)", "Vector(1, 1)", {}),
    "anchorRatio": ("Vector(0, 0)", "Vector(1, 1)", {}),
    "scale": ("1", "2", {}),
    "radius": ("0.4", "1.4", {}),
    "rx": ("0.4", "1.6", {}),
    "ry": ("0.3", "1.4", {}),
    "w": ("1", "3", {}),
    "h": ("0.4", "2", {}),
    "from": ("Vector(0.5, 0.5)", "Vector(0.5, 3.5)", {}),
    "to": ("Vector(3.5, 3.5)", "Vector(3.5, 0.5)", {}),
    "points": ("[Vector(0.5, 0.5), Vector(3.5, 1), Vector(2, 3.5)]", "[Vector(1, 1), Vector(3, 1), Vector(2, 3)]", {}),
    "segments": ("[(:line, Vector(3.5, 2)), (:line, Vector(1, 3.5))]", "[(:line, Vector(3.5, 3.5)), (:line, Vector(0.5, 3))]", {}),
    "closed": ("false", "true", {}),
    "upto": ("0.3", "1", {}),
    "text": ('"Ag"', '"Xy"', {}),
    "font": ('"Helvetica"', '"Courier"', {}),
    "fontSize": ("0.6", "1.6", {}),
    # 左上を (0, 2) に置いて、幅 4 の中で寄せる
    "align": (":left", ":right", {"w": "4", "position": "Vector(0, 2)", "anchor": "Vector(0, 0)"}),
    "fill": ("#e04040", "#4040e0", {}),
    "stroke": ("#40e040", "#ffff00", {"strokeWidth": "0.2"}),
    "strokeWidth": ("0.05", "0.4", {"stroke": "#40e040"}),
    "strokeCap": (":butt", ":round", {"stroke": "#40e040", "strokeWidth": "0.3", "dash": "[0.4, 0.4]"}),
    "strokeJoin": (":miter", ":round", {"stroke": "#40e040", "strokeWidth": "0.4"}),
    "dash": ("[]", "[0.3, 0.3]", {"stroke": "#40e040", "strokeWidth": "0.2"}),
    "dashOffset": ("0", "0.15", {"stroke": "#40e040", "strokeWidth": "0.2", "dash": "[0.3, 0.3]"}),
    "opacity": ("1", "0.2", {}),
    "rotation": ("0", "0.7", {}),
    "blend": (":normal", ":multiply", {}),
}

# Group は中の円が枠 (切り口) からはみ出す形にして見る。Group の位置は (1, 0.5)
GROUP_BASE = {"position": "Vector(1, 0.5)"}
# (属性, 値 1, 値 2, 一緒に書いておく属性)
GROUP_CASES = [
    ("position", "Vector(0.2, 0.5)", "Vector(1, 0.5)", {}),
    ("anchor", "Vector(0, 0)", "Vector(1, 1)", {"rotation": "0.5"}),
    ("scale", "1", "1.4", {}),
    ("rotation", "0", "0.5", {}),
    ("opacity", "1", "0.2", {}),
    ("blend", ":normal", ":multiply", {}),
    ("camera", "Camera(from = Vector(1, 1), to = Vector(1.5, 1.5), scale = 1)", "Camera(from = Vector(1, 1), to = Vector(1.5, 1.5), scale = 2)", {}),
    ("clip", "Rect(w = 3, h = 3)", "Circle(radius = 0.8)", {}),
]

# 入れ子の Composition。box は長さの単位 (vx) を通して効くので、中の円の半径を 30vx にして見る
COMPOSITION_BASE = {"box": "Vector(3, 3)", "duration": "1s", "position": "Vector(1, 0.5)"}
COMPOSITION_CASES = [
    ("box", "Vector(3, 3)", "Vector(6, 3)", {}),
    ("position", "Vector(0.2, 0.5)", "Vector(1, 0.5)", {}),
    ("anchor", "Vector(0, 0)", "Vector(1, 1)", {"rotation": "0.5"}),
    ("scale", "1", "1.4", {}),
    ("rotation", "0", "0.5", {}),
    ("opacity", "1", "0.2", {}),
    ("blend", ":normal", ":multiply", {}),
]
# 絵に出ない属性。長さは動画の長さを決めるだけで、入れ子にした絵には効かない
NOT_DRAWN = {("Composition", "duration")}


# World と Solid は図形ではなく塗りなので別に書く。Rect の fill に入れて描く
WORLD_BASE = {"camera": "space3d.PerspectiveCamera(from = (3, 2.5, 5), to = (0, 0, 0), fov = 0.7, box = (4, 4))", "parts": "[solid]"}
SOLID_BASE = {"mesh": "space3d.box((0, 0, 0), (2, 2, 2))"}
# (型, 属性, 値 1, 値 2)
PAINT_CASES = [
    ("World", "camera", WORLD_BASE["camera"], "space3d.PerspectiveCamera(from = (3, 2.5, 5), to = (0, 0, 0), fov = 0.45, box = (4, 4))"),
    ("World", "parts", "[solid]", "[]"),
    ("World", "light", "(0.4, 0.8, 0.5)", "(-0.7, 0.2, 0.5)"),
    ("World", "ambient", "0.1", "0.8"),
    ("World", "background", "#101010", "#2060a0"),
    ("Solid", "mesh", SOLID_BASE["mesh"], "space3d.sphere((0, 0, 0), 1.2, 8, 16)"),
    ("Solid", "fill", "#e04040", "#4040e0"),
    ("Solid", "transform", "space3d.Transform3()", "space3d.Transform3().rotate_y(35)"),
]

# 背の四角。どの絵にも敷く
BACKGROUND = 'comp.place("bg", Rect(w = 4, h = 4, anchor = Vector(0, 0), fill = #303030), in = 0s, out = 1s)\n'


def render(src_text: str, work: Path) -> str:
    """スクリプトを 0 秒で描いて、絵の指紋を返す。描けなければ ERR: ..."""
    src, out = work / "case.moph", work / "case.png"
    src.write_text(src_text)
    r = subprocess.run([BIN, "render", str(src), "-o", str(out), "--at", "0s"], capture_output=True, text=True)
    if r.returncode != 0:
        return "ERR: " + (r.stderr.strip().splitlines() or ["?"])[-1]
    return hashlib.sha1(out.read_bytes()).hexdigest()


def args_of(attrs: dict) -> str:
    return ", ".join(f"{k} = {v}" for k, v in attrs.items())


def draw_world(kind: str, attr: str, value: str, work: Path) -> str:
    """Rect の fill に World を入れて描く。attr は World か Solid のどちらかに書き足す"""
    world, solid = dict(WORLD_BASE), dict(SOLID_BASE)
    (world if kind == "World" else solid)[attr] = value
    return render(
        "import space3d\n"
        "let comp = Composition(box = Vector(4, 4), duration = 1s)\n"
        f"let solid = space3d.Solid({args_of(solid)})\n"
        f'comp.place("world", Rect(w = 4, h = 4, anchor = Vector(0, 0), fill = space3d.World({args_of(world)})), in = 0s, out = 1s)\n'
        "output comp\n",
        work,
    )


def draw_group(attrs: dict, work: Path) -> str:
    """Group に入れた円を描く。円は Group の枠からはみ出す位置にある"""
    return render(
        "let comp = Composition(box = Vector(4, 4), duration = 1s)\n"
        + BACKGROUND
        + f"let g = Group(Circle(position = Vector(0, 1.5), radius = 1, fill = #e04040), {args_of(attrs)})\n"
        'comp.place("g", g, in = 0s, out = 1s)\n'
        "output comp\n",
        work,
    )


def draw_nested(attrs: dict, work: Path) -> str:
    """入れ子の Composition に円を置いて描く。円の半径は箱の幅の 30%"""
    return render(
        "let comp = Composition(box = Vector(4, 4), duration = 1s)\n"
        + BACKGROUND
        + f"let inner = Composition({args_of(attrs)})\n"
        'inner.place("dot", Circle(position = Vector(0, 1.5), radius = 30vx, fill = #e04040), in = 0s, out = 1s)\n'
        'comp.place("inner", inner, in = 0s, out = 1s)\n'
        "output comp\n",
        work,
    )


def draw(kind: str, attrs: dict, work: Path) -> str:
    return render(
        "let comp = Composition(box = Vector(4, 4), duration = 1s)\n"
        + BACKGROUND
        + f'comp.place("shape", {kind}({args_of(attrs)}), in = 0s, out = 1s)\n'
        "output comp\n",
        work,
    )


def differs(label: str, a: str, b: str, failed: list[str]) -> None:
    if a.startswith("ERR") or b.startswith("ERR"):
        failed.append(f"{label}: {a if a.startswith('ERR') else b}")
    elif a == b:
        failed.append(f"{label}: 値を変えても絵が変わらない")


def check_required(doc: dict, work: Path) -> tuple[int, list[str]]:
    """必須の印が描画側と合っているか。省くと描けない / 必須だけで描ける"""
    required = {t["name"]: [a["name"] for a in t.get("attrs", []) if a.get("required")] for t in doc["types"]}
    failed, checked = [], 0
    for kind, base in BASE.items():
        need = required.get(kind, [])
        checked += 1
        only = {k: v for k, v in base.items() if k in need}
        if len(only) != len(need):
            failed.append(f"{kind}: 必須と書いた属性の値が scripts/check_attrs.py に無い: {' '.join(sorted(set(need) - set(only)))}")
        elif draw(kind, only, work).startswith("ERR"):
            failed.append(f"{kind}: 必須の属性だけでは描けない (必須の印が足りない)")
        for attr in need:
            checked += 1
            if not draw(kind, {k: v for k, v in base.items() if k != attr}, work).startswith("ERR"):
                failed.append(f"{kind}.{attr}: 必須と書いてあるが、省いても描ける")
    # Composition は box と duration が必須
    for attr in required.get("Composition", []):
        checked += 1
        rest = {k: v for k, v in COMPOSITION_BASE.items() if k != attr}
        if not draw_nested(rest, work).startswith("ERR"):
            failed.append(f"Composition.{attr}: 必須と書いてあるが、省いても描ける")
    checked += 1
    if draw_nested({k: v for k, v in COMPOSITION_BASE.items() if k in required.get("Composition", [])}, work).startswith("ERR"):
        failed.append("Composition: 必須の属性だけでは描けない (必須の印が足りない)")
    return checked, failed


def main() -> int:
    doc = json.loads(subprocess.run([BIN, "doc"], capture_output=True, text=True).stdout)
    types = {t["name"]: [a["name"] for a in t.get("attrs", [])] for t in doc["types"]}
    failed, checked = [], 0
    with tempfile.TemporaryDirectory() as d:
        work = Path(d)
        for kind, base in BASE.items():
            for attr in types.get(kind, []):
                checked += 1
                if attr not in TRY:
                    failed.append(f"{kind}.{attr}: 試す値が scripts/check_attrs.py に無い")
                    continue
                a, b, extra = TRY[attr]
                setup = {**base, **extra}
                differs(f"{kind}.{attr}", draw(kind, {**setup, attr: a}, work), draw(kind, {**setup, attr: b}, work), failed)
        # Group
        for attr, a, b, extra in GROUP_CASES:
            checked += 1
            differs(f"Group.{attr}", draw_group({**GROUP_BASE, **extra, attr: a}, work), draw_group({**GROUP_BASE, **extra, attr: b}, work), failed)
        missing = [a for a in types.get("Group", []) if a not in {c[0] for c in GROUP_CASES}]
        if missing:
            failed.append(f"Group: 試す値が scripts/check_attrs.py に無い: {' '.join(missing)}")
        # 入れ子の Composition
        for attr, a, b, extra in COMPOSITION_CASES:
            checked += 1
            differs(f"Composition.{attr}", draw_nested({**COMPOSITION_BASE, **extra, attr: a}, work), draw_nested({**COMPOSITION_BASE, **extra, attr: b}, work), failed)
        missing = [a for a in types.get("Composition", []) if a not in {c[0] for c in COMPOSITION_CASES} and ("Composition", a) not in NOT_DRAWN]
        if missing:
            failed.append(f"Composition: 試す値が scripts/check_attrs.py に無い: {' '.join(missing)}")
        # 3D の塗りも同じように見る
        for kind, attr, a, b in PAINT_CASES:
            checked += 1
            differs(f"{kind}.{attr}", draw_world(kind, attr, a, work), draw_world(kind, attr, b, work), failed)
        for kind in ("World", "Solid"):
            missing = [a for a in types.get(kind, []) if a not in {c[1] for c in PAINT_CASES if c[0] == kind}]
            if missing:
                failed.append(f"{kind}: 試す値が scripts/check_attrs.py に無い: {' '.join(missing)}")
        n, bad = check_required(doc, work)
        checked += n
        failed += bad
    print(f"attrs: {checked - len(failed)}/{checked} ok")
    for line in failed:
        print("  " + line)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
