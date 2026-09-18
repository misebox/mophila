#!/usr/bin/env python3
"""バージョンを 1 つ上げる。

    scripts/bump-version.py            patch を 1 つ上げる (既定)
    scripts/bump-version.py minor      minor を 1 つ上げて patch を 0 に
    scripts/bump-version.py major      major を 1 つ上げて minor と patch を 0 に
    scripts/bump-version.py --dry-run  書き換えずに、上げた結果だけ出す
    scripts/bump-version.py --tag      いまの版で HEAD に v0.1.5 のタグを打つ (上げない)

Cargo.toml が正で、Cargo.lock と editors/vscode/package.json を同じ値に揃える。
上げるのは実行ファイルの中身が変わったときだけ。文書や skill の修正では上げない。

使う順: bump → コミット → --tag。commit と push はしない。
"""
import argparse
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CARGO = ROOT / "Cargo.toml"
LOCK = ROOT / "Cargo.lock"
VSCODE = ROOT / "editors/vscode/package.json"


def current() -> tuple[int, int, int]:
    m = re.search(r'^version = "(\d+)\.(\d+)\.(\d+)"', CARGO.read_text(), re.M)
    if not m:
        sys.exit(f"{CARGO.name} に version = \"x.y.z\" が無い")
    return int(m[1]), int(m[2]), int(m[3])


def bumped(part: str, v: tuple[int, int, int]) -> tuple[int, int, int]:
    major, minor, patch = v
    if part == "major":
        return major + 1, 0, 0
    if part == "minor":
        return major, minor + 1, 0
    return major, minor, patch + 1


def replace(path: Path, pattern: str, new: str, dry: bool) -> str:
    text = path.read_text()
    out, n = re.subn(pattern, new, text, count=1, flags=re.M)
    if n != 1:
        sys.exit(f"{path.relative_to(ROOT)}: 書き換える行が見つからない ({pattern})")
    if not dry:
        path.write_text(out)
    return f"{path.relative_to(ROOT)}"


def git(*args: str) -> str:
    return subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True, check=True).stdout.strip()


def tag() -> int:
    """いまの版で HEAD にタグを打つ。コミット済みの版と食い違っていたら止める"""
    version = ".".join(map(str, current()))
    name = f"v{version}"
    if git("status", "--porcelain", "Cargo.toml"):
        return fail(f"Cargo.toml に未コミットの変更がある。先にコミットする")
    committed = git("show", "HEAD:Cargo.toml")
    if f'version = "{version}"' not in committed:
        return fail(f"HEAD の Cargo.toml が {version} でない。バージョンを上げたコミットに打つ")
    # 上げたコミットそのものに打つ。あとから打つと、関係ないコミットに版が付く
    before = git("show", "HEAD~1:Cargo.toml")
    if f'version = "{version}"' in before:
        return fail(f"HEAD は {version} に上げたコミットではない。上げた直後に打つ")
    if name in git("tag", "--list", name).splitlines():
        return fail(f"{name} は既にある ({git('rev-parse', '--short', name)})")
    git("tag", "-a", name, "-m", name)
    print(f"{name} -> {git('rev-parse', '--short', 'HEAD')}  ({git('log', '-1', '--format=%s')})")
    print("push はしない")
    return 0


def fail(message: str) -> int:
    print(message, file=sys.stderr)
    return 1


def main() -> int:
    ap = argparse.ArgumentParser(description="バージョンを 1 つ上げる")
    ap.add_argument("part", nargs="?", default="patch", choices=["major", "minor", "patch"])
    ap.add_argument("-n", "--dry-run", action="store_true", help="書き換えずに結果だけ出す")
    ap.add_argument("--tag", action="store_true", help="上げずに、いまの版で HEAD にタグを打つ")
    args = ap.parse_args()
    if args.tag:
        return tag()

    old = current()
    new = bumped(args.part, old)
    old_s = ".".join(map(str, old))
    new_s = ".".join(map(str, new))

    touched = [
        replace(CARGO, rf'^version = "{re.escape(old_s)}"', f'version = "{new_s}"', args.dry_run),
        # Cargo.lock の mophila の行。他の crate を巻き込まないように name の直後だけを見る
        replace(LOCK, rf'^(name = "mophila"\nversion = )"{re.escape(old_s)}"', rf'\g<1>"{new_s}"', args.dry_run),
        replace(VSCODE, rf'^(  "version": )"{re.escape(old_s)}"', rf'\g<1>"{new_s}"', args.dry_run),
    ]
    print(f"{old_s} -> {new_s}{' (dry run)' if args.dry_run else ''}")
    for t in touched:
        print(f"  {t}")

    if not args.dry_run:
        # Cargo.lock が本当に揃ったかは cargo に聞く (ビルドはしない)
        r = subprocess.run(["cargo", "metadata", "--no-deps", "--offline", "--format-version", "1"], cwd=ROOT, capture_output=True, text=True)
        if r.returncode != 0:
            print("  cargo metadata が通らない。Cargo.lock を確かめる", file=sys.stderr)
            return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
