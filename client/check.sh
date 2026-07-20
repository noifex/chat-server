#!/usr/bin/env bash
# check.sh — workspace を build & test する静的封印（②）。
#
# persona は固定コマンド `bash ../../check.sh` だけ叩く（動的な $() を内包＝allowlist可）。
# cwd を workspace に固定＝どこから呼ばれても検証対象は workspace のみ（範囲＝爆風半径）。
# 終了コードで pass(0)/fail を返す＝orchestrator が subprocess で読める（Step17 の検証終端）。
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
ws="$here/workspace"
cd "$ws" || { echo "check.sh: workspace が無い ($ws)" >&2; exit 2; }

# 今は Rust(cargo)決め打ち（Step18 FizzBuzz が Rust）。
# 多言語化は将来 Cargo.toml/package.json/go.mod 等で自動判定（persona側コマンドは固定のまま）。
[ -f Cargo.toml ] || { echo "check.sh: workspace に Cargo.toml 無し（agentがまだ何も作ってない）" >&2; exit 3; }

cargo build && cargo test
