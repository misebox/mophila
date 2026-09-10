#!/usr/bin/env python3
"""宣言した属性が、本当に絵に効くかを確かめる。

`mophila doc` が挙げる図形の属性を 2 通りの値で描いて、絵が変わることを見る。
変わらなければ「代入できるのに何も起こらない」属性なので、描画側を直すか宣言から外す。
属性を足したのにここに書き方が無ければ、それも失敗として出る。

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

# 図形ごとの、書かないと描けない属性
BASE = {
    "Circle": {"position": "Pos(2, 2)", "radius": "1", "fill": "#e04040"},
    "Ellipse": {"position": "Pos(2, 2)", "rx": "1.5", "ry": "0.5", "fill": "#e04040"},
    "Rect": {"position": "Pos(2, 2)", "w": "2", "h": "1", "fill": "#e04040"},
    "Line": {"from": "Vector(0.5, 0.5)", "to": "Vector(3.5, 3.5)", "stroke": "#e04040", "strokeWidth": "0.3"},
    "Polygon": {"points": "[Vector(0.5, 0.5), Vector(3.5, 1), Vector(2, 3.5)]", "fill": "#e04040"},
    "Path": {"from": "Vector(0.5, 0.5)", "segments": "[(:line, Vector(3.5, 2)), (:line, Vector(1, 3.5))]", "stroke": "#e04040", "strokeWidth": "0.4"},
    "TextArea": {"position": "Pos(2, 2)", "text": '"Ag"', "fontSize": "1.5", "fill": "#e04040"},
}

# 属性ごとの、試す 2 つの値と、効き目を見るために一緒に書いておく属性
TRY = {
    "position": ("Pos(1, 1)", "Pos(3, 3)", {}),
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
    "text": ('"Ag"', '"Xy"', {}),
    "font": ('"Helvetica"', '"Courier"', {}),
    "fontSize": ("0.6", "1.6", {}),
    "align": (":left", ":right", {"w": "4", "position": "Pos(0, 2, anchor = :left)"}),
    "fill": ("#e04040", "#4040e0", {}),
    "stroke": ("#40e040", "#ffff00", {"strokeWidth": "0.2"}),
    "strokeWidth": ("0.05", "0.4", {"stroke": "#40e040"}),
    "strokeCap": (":butt", ":round", {"stroke": "#40e040", "strokeWidth": "0.3", "dash": "[0.4, 0.4]"}),
    "strokeJoin": (":miter", ":round", {"stroke": "#40e040", "strokeWidth": "0.4"}),
    "dash": ("[]", "[0.3, 0.3]", {"stroke": "#40e040", "strokeWidth": "0.2"}),
    "dashOffset": ("0", "0.15", {"stroke": "#40e040", "strokeWidth": "0.2", "dash": "[0.3, 0.3]"}),
    "opacity": ("1", "0.2", {}),
    "rotation": ("0", "40", {}),
    "pivot": ("Vector(0, 0)", "Vector(2, 2)", {"rotation": "40"}),
    "blend": (":normal", ":multiply", {}),
}


def draw(kind: str, attrs: dict, work: Path) -> str:
    body = ", ".join(f"{k} = {v}" for k, v in attrs.items())
    src, out = work / "case.moph", work / "case.png"
    src.write_text(
        "let v = View(box = Vector(4, 4))\n"
        "v.place(Rect(position = Pos(0, 0, anchor = :topLeft), w = 4, h = 4, fill = #303030))\n"
        f"v.place({kind}({body}))\n"
        "output v\n"
    )
    r = subprocess.run([BIN, "render", str(src), "-o", str(out), "--at", "0s"], capture_output=True, text=True)
    if r.returncode != 0:
        return "ERR: " + (r.stderr.strip().splitlines() or ["?"])[-1]
    return hashlib.sha1(out.read_bytes()).hexdigest()


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
                ha = draw(kind, {**setup, attr: a}, work)
                hb = draw(kind, {**setup, attr: b}, work)
                if ha.startswith("ERR") or hb.startswith("ERR"):
                    failed.append(f"{kind}.{attr}: {ha if ha.startswith('ERR') else hb}")
                elif ha == hb:
                    failed.append(f"{kind}.{attr}: 値を変えても絵が変わらない")
    print(f"attrs: {checked - len(failed)}/{checked} ok")
    for line in failed:
        print("  " + line)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
