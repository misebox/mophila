#!/usr/bin/env bash
# バージョンを上げて、コミットして、タグを打つ。
#
#   scripts/bump-version.sh 0.3.0     この版にする (patch / minor / major でも可)
#   scripts/bump-version.sh --show    いまの版を出すだけ
#
# Cargo.toml が正で、Cargo.lock と editors/vscode の package.json / package-lock.json を同じ値に揃える。
# 他に変更があるときは何もしない (版だけのコミットにするため)。push はしない。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CARGO="Cargo.toml"
LOCK="Cargo.lock"
VSCODE="editors/vscode/package.json"
VSCODE_LOCK="editors/vscode/package-lock.json"
cd "$ROOT"

die() { echo "$*" >&2; exit 1; }

now="$(sed -n '/^\[package\]/,/^\[/ s/^version = "\(.*\)"/\1/p' "$CARGO" | head -1)"
[ -n "$now" ] || die "$CARGO に version が無い"

case "${1:---show}" in
  --show)
    echo "$now"
    exit 0
    ;;
  patch|minor|major)
    IFS=. read -r major minor patch <<< "$now"
    case "$1" in
      major) new="$((major + 1)).0.0" ;;
      minor) new="$major.$((minor + 1)).0" ;;
      patch) new="$major.$minor.$((patch + 1))" ;;
    esac
    ;;
  [0-9]*.[0-9]*.[0-9]*)
    new="$1"
    ;;
  *)
    die "使い方: scripts/bump-version.sh 0.3.0   (patch / minor / major でも可)"
    ;;
esac

# --- 書く前に全部確かめる。1 つでも駄目なら、何も書かずに終わる ---
[ -z "$(git status --porcelain --untracked-files=no)" ] ||
  die "ほかに変更がある。先にコミットするか戻す (版だけのコミットにしたい)
$(git status --short --untracked-files=no)"
git rev-parse --verify --quiet "v$new" >/dev/null && die "タグ v$new は既にある"
[ "$now" != "$new" ] || die "いまも $new になっている"
grep -q "^version = \"$now\"" "$CARGO" || die "$CARGO が $now でない"
grep -q "^name = \"mophila\"$" "$LOCK" || die "$LOCK に mophila が無い"
grep -q "^  \"version\": \"$now\"" "$VSCODE" || die "$VSCODE が $now でない"
# package-lock.json は頭の方に 2 か所 (それ自身と packages の "")。依存の版は後ろにあるので巻き込まない
[ "$(head -12 "$VSCODE_LOCK" | grep -c "^ *\"version\": \"$now\",$")" -eq 2 ] || die "$VSCODE_LOCK が $now でない"

# --- 書く ---
write() { # ファイル, 新しい中身を作るコマンド
  local file="$1" tmp
  shift
  tmp="$(mktemp)"
  "$@" "$file" > "$tmp"
  mv "$tmp" "$file"
}

write "$CARGO" sed "1,/^\[dependencies\]/ s/^version = \"$now\"/version = \"$new\"/"
write "$VSCODE" sed "s/^\\(  \"version\": \\)\"$now\"/\\1\"$new\"/"
write "$VSCODE_LOCK" sed "1,12 s/^\\( *\"version\": \\)\"$now\",$/\\1\"$new\",/"
# Cargo.lock は mophila の行の直後の version だけ (同じ版の別の crate を巻き込まない)
write "$LOCK" awk -v old="$now" -v new="$new" '
  /^name = "mophila"$/ { mine = 1 }
  mine && $0 == "version = \"" old "\"" { print "version = \"" new "\""; mine = 0; next }
  { print }
'

# --- コミットしてタグを打つ ---
git add "$CARGO" "$LOCK" "$VSCODE" "$VSCODE_LOCK"
git commit -q -m "Raise the version to $new"
git tag -a "v$new" -m "v$new"
echo "$now -> $new  ($(git rev-parse --short HEAD), v$new)"
echo "push はしない"
