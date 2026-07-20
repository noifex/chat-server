#!/usr/bin/env bash
# tick.sh <Name> — 1tick分の判断材料を1コマンドで返す。
#   出力 "STOP"            → 会話終了。persona は /loop を止める
#   出力 "SKIP (turn=X)"   → 自分の番じゃない。何もしない（cursorも進めない）
#   出力 "YOUR TURN..."+新着 → 自分の番。新着を読んで返事する
# $() を script 内に封印して persona が打つコマンドを静的化（allowlist可）。
# 実行: persona dir（clients/<Name>）から `bash ../../tick.sh <Name>`
name="$1"
turn=$(cat ../turn 2>/dev/null || echo none)
[ "$turn" = "STOP" ] && { echo "STOP"; exit 0; }
if [ "$turn" != "$name" ]; then
    timeout 60 cat "$name.notify" >/dev/null 2>&1   # 起こされる or 60sで諦め（turn fileの再readが真実）
    turn=$(cat ../turn 2>/dev/null || echo none)
    [ "$turn" = "STOP" ] && { echo "STOP"; exit 0; }
    [ "$turn" != "$name" ] && { echo "SKIP (turn=$turn)"; exit 0; }
fi
echo "YOUR TURN. new:"
r=$(cat "$name.cursor" 2>/dev/null || echo 0); r=$((r + 0))   # 既読行数（空白除去）
tail -n "+$((r + 1))" "$name.inbox"                          # 既読の次から新着
awk 'END{print NR}' "$name.inbox" > "$name.cursor"           # cursor更新（自分の番の時だけ）
