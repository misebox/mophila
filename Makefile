# 手で覚える順番を減らすための入口。中身は cargo / scripts / site に任せる
.PHONY: all docs site check media dev install-skill bump bump-minor bump-major

# 既定: 検査してからドキュメント一式を作る
all: check docs

## ドキュメント一式。data.json も llms.txt も site/dist もこれだけで揃う
docs:
	cd site && bun run build

## ドキュメントページをその場で見る
dev:
	cd site && bun run dev

## サンプルの動画も作り直す (GPU が要る。時間がかかる)
media:
	cargo build
	python3 scripts/docgen.py --media
	cd site && bun run build

## 書き方の skill を ~/.claude/skills に複製する (変えたら入れ直す)
install-skill:
	scripts/install-skill.sh

## バージョンを上げて、コミットして、タグを打つ (一番下の桁)
bump:
	scripts/bump-version.sh patch

## 真ん中の桁を上げる
bump-minor:
	scripts/bump-version.sh minor

## 一番上の桁を上げる
bump-major:
	scripts/bump-version.sh major

## 全部の検査
check:
	cargo build
	cargo test
	python3 scripts/check_examples.py
	python3 scripts/check_attrs.py
