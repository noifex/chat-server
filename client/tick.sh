#!/usr/bin/env bash
# tick.sh <Name> — 1tick分の判断材料を1コマンドで返す。
#   出力 "STOP"            → 会話終了。persona は /loop を止める
#   出力 "SKIP (turn=X)"   → 自分の番じゃない。何もしない（cursorも進めない）
#   出力 "YOUR TURN..."+新着 → 自分の番。新着を読んで返事する
# cursor はここでは確定しない。表示範囲を .cursor.pending に保存し、
# say.sh が FIFO への書き込みに成功した後でだけ .cursor に確定する。
# $() を script 内に封印して persona が打つコマンドを静的化（allowlist可）。
# 実行: persona dir（clients/<Name>）から `bash ../../tick.sh <Name>`
name="$1"
here="$(cd "$(dirname "$0")" && pwd)"
turn=$(cat ../turn 2>/dev/null || echo none)
[ "$turn" = "STOP" ] && { echo "STOP"; exit 0; }
if [ "$turn" != "$name" ]; then
    python3 "$here/fifo_io.py" wait "$name.notify" 60 >/dev/null 2>&1
    turn=$(cat ../turn 2>/dev/null || echo none)
    [ "$turn" = "STOP" ] && { echo "STOP"; exit 0; }
    [ "$turn" != "$name" ] && { echo "SKIP (turn=$turn)"; exit 0; }
fi
r=$(cat "$name.cursor" 2>/dev/null || echo 0)
case "$r" in
    ''|*[!0-9]*) echo "tick.sh: invalid cursor: $r" >&2; exit 1;;
esac

# 行数を先に固定する。直後に追記された行は今回表示も ack もせず、次回に回す。
total=$(awk 'END{print NR + 0}' "$name.inbox")
[ "$r" -le "$total" ] || { echo "tick.sh: cursor $r exceeds inbox length $total" >&2; exit 1; }

pending="$name.cursor.pending"
pending_tmp=$(mktemp "${pending}.tmp.XXXXXX") || exit 1
trap 'rm -f "$pending_tmp"' EXIT
printf '%s\n' "$total" > "$pending_tmp"
mv "$pending_tmp" "$pending"
trap - EXIT

echo "YOUR TURN. new:"
if [ "$r" -lt "$total" ]; then
    sed -n "$((r + 1)),${total}p" "$name.inbox"
fi
