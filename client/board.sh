#!/usr/bin/env bash
# board.sh — .runtime/board.wal を使って board binary を叩く薄いラッパ。
# 長い BOARD_WAL / binary パスを毎回打たなくて済む（全角スペース事故の根絶）。
# 使い方（chat-server-client から）:
#   bash board.sh project
#   bash board.sh add "x"
#   bash board.sh claim 1 Coffee
here="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$here/.runtime"                # fresh clone でも lock/wal を置けるように
BOARD_WAL="$here/.runtime/board.wal" "$here/../board/target/debug/board" "$@"
