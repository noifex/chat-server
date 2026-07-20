#!/usr/bin/env bash
# setup.sh — 各 persona の settings.local.json を settings.example.json から生成する。
# fresh clone 後に1回叩く。settings.local.json は .gitignore 済み（machine 固有）。
#   使い方: cd client && ./setup.sh
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
tpl="$here/clients/settings.example.json"

for name in Coffee Cola Tea; do
    dir="$here/clients/$name/.claude"
    mkdir -p "$dir"
    # _comment 行を落とし、NAME を実名に置換
    sed -e '/_comment/d' -e "s/NAME/$name/g" "$tpl" > "$dir/settings.local.json"
    echo "generated: clients/$name/.claude/settings.local.json"
done
# workspace（agent 成果物の隔離 git repo）を用意 — saga/revert の安全網。
# 外側 repo からは .gitignore 済み＝独立した nested repo。
ws="$here/workspace"
mkdir -p "$ws"
if [ ! -d "$ws/.git" ]; then
    ( cd "$ws" && git init -q && git commit -q --allow-empty -m "workspace init" )
    echo "initialized: client/workspace/ (隔離 git repo — saga/revert の安全網)"
fi

echo "done — persona ごとに settings.local.json を生成、workspace を初期化した（Codex は codex CLI 用の AGENTS.md を使う）。"
