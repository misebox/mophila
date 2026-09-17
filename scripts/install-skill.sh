#!/usr/bin/env bash
# docs/skills/mophila を Claude Code の skills に入れる。
#
#   scripts/install-skill.sh              # symlink を張る (リポジトリを直せばそのまま反映される)
#   scripts/install-skill.sh --copy       # その時点の中身を複製する
#   scripts/install-skill.sh --uninstall  # 外す
#
# 入れ先は $CLAUDE_CONFIG_DIR/skills、無ければ ~/.claude/skills。
set -euo pipefail

NAME=mophila
SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/docs/skills/$NAME"
SKILLS="${CLAUDE_CONFIG_DIR:-$HOME/.claude}/skills"
DEST="$SKILLS/$NAME"
mode=link
force=no

for arg in "$@"; do
  case "$arg" in
    --copy) mode=copy ;;
    --uninstall) mode=uninstall ;;
    --force) force=yes ;;
    -h|--help) sed -n '2,8p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

if [ ! -f "$SRC/SKILL.md" ]; then
  echo "skill not found: $SRC/SKILL.md" >&2
  exit 1
fi

if [ "$mode" = uninstall ]; then
  if [ -L "$DEST" ]; then
    rm "$DEST"
    echo "removed $DEST"
  elif [ -d "$DEST" ]; then
    if [ "$force" != yes ]; then
      echo "$DEST is a real directory. --force を付けると消す" >&2
      exit 1
    fi
    rm -r "$DEST"
    echo "removed $DEST"
  else
    echo "not installed: $DEST"
  fi
  exit 0
fi

mkdir -p "$SKILLS"

# 既にあるものを見てから決める。中身のあるディレクトリは --force なしでは消さない
if [ -L "$DEST" ]; then
  current="$(readlink "$DEST")"
  if [ "$mode" = link ] && [ "$current" = "$SRC" ]; then
    echo "already linked: $DEST -> $SRC"
    exit 0
  fi
  rm "$DEST"
elif [ -e "$DEST" ]; then
  if [ "$force" != yes ]; then
    echo "$DEST already exists (symlink ではない)。入れ替えるなら --force" >&2
    exit 1
  fi
  rm -r "$DEST"
fi

if [ "$mode" = link ]; then
  ln -s "$SRC" "$DEST"
  echo "linked $DEST -> $SRC"
else
  cp -R "$SRC" "$DEST"
  echo "copied $SRC -> $DEST"
fi

echo "Claude Code を起動し直すと /$NAME が使える"
