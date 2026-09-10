#!/usr/bin/env python3
"""examples/ の全スクリプトが実行でき、examples/syntax/error.moph の各行が
書いてある種別のエラーで止まることを確かめる。本体のテストではなく、言語や部品を変えたときに手で回す。

使い方: scripts/check_examples.py   (先に cargo build)
"""
import re
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BIN = ROOT / "target/debug/mophila"


def run(path: Path) -> subprocess.CompletedProcess:
    return subprocess.run([BIN, "run", str(path)], capture_output=True, text=True)


def check_scripts() -> list[str]:
    files = sorted(p for p in (ROOT / "examples/syntax").glob("*.moph") if p.name != "error.moph")
    files += sorted((ROOT / "examples/gallery").glob("*.moph"))
    files += sorted((ROOT / "examples/gallery").glob("*/main.moph"))
    failed = []
    for f in files:
        r = run(f)
        if r.returncode != 0:
            failed.append(f"{f.relative_to(ROOT)}: {r.stderr.strip().splitlines()[-1] if r.stderr.strip() else '(no output)'}")
    print(f"scripts: {len(files) - len(failed)}/{len(files)} ok")
    return failed


def check_errors() -> list[str]:
    src = (ROOT / "examples/syntax/error.moph").read_text().splitlines()
    setup, failed, checked = [], [], 0
    with tempfile.TemporaryDirectory() as d:
        case = Path(d) / "case.moph"
        for line in src:
            if line.lstrip().startswith("#"):
                continue
            code = re.split(r"\s+#\s", line)[0].rstrip()
            if not code:
                continue
            m = re.search(r"#\s*((?:Syntax|Name|Type|Value|Runtime)Error\.\w+)", line)
            if not m:
                setup.append(code)
                continue
            checked += 1
            case.write_text("\n".join(setup + [code]) + "\n")
            r = run(case)
            last = r.stderr.strip().splitlines()[-1] if r.stderr.strip() else "(no error)"
            if not last.startswith(f"Error: {m.group(1)}"):
                failed.append(f"{code[:45]:<45} expected {m.group(1)}, got: {last}")
    print(f"error kinds: {checked - len(failed)}/{checked} ok")
    return failed


if __name__ == "__main__":
    failed = check_scripts() + check_errors()
    for f in failed:
        print("  NG", f)
    sys.exit(1 if failed else 0)
