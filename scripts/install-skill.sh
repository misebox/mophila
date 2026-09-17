#!/usr/bin/env bash
# docs/skills/mophila を Claude Code の skills にインストールする (複製する)。
#
#   scripts/install-skill.sh              # 入れる。既に入っていれば入れ直す
#   scripts/install-skill.sh --uninstall  # 外す
#
# 入れ先は $CLAUDE_CONFIG_DIR/skills、無ければ ~/.claude/skills。
# リポジトリの中身を変えたら、もう一度走らせて入れ直す。
set -euo pipefail

NAME=mophila
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="$ROOT/docs/skills/$NAME"
SKILLS="${CLAUDE_CONFIG_DIR:-$HOME/.claude}/skills"
DEST="$SKILLS/$NAME"
mode=install

for arg in "$@"; do
  case "$arg" in
    --uninstall) mode=uninstall ;;
    -h|--help) sed -n '2,8p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

if [ ! -f "$SRC/SKILL.md" ]; then
  echo "skill not found: $SRC/SKILL.md" >&2
  exit 1
fi

if [ "$mode" = uninstall ]; then
  if [ -e "$DEST" ] || [ -L "$DEST" ]; then
    rm -r "$DEST"
    echo "removed $DEST"
  else
    echo "not installed: $DEST"
  fi
  exit 0
fi

mkdir -p "$SKILLS"
# 前に入れたもの (以前の版が張った symlink も含む) を消してから入れる
rm -rf "$DEST"
cp -R "$SRC" "$DEST"

version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
echo "installed mophila $version skill -> $DEST"
find "$DEST" -type f -name '*.md' | sed "s|$DEST/|  |" | sort
echo "Claude Code を起動し直すと /$NAME が使える。中身を変えたら、もう一度これを走らせる"
