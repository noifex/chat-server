#!/usr/bin/env bash
# inbox.sh <Name> — 新着を出力して cursor を更新する。
# 目的: command substitution $() を script 内に封じ込め、persona が打つコマンドを
#       静的にする（→ Claude Code の allowlist で事前許可でき、毎回の承認が消える）。
# 実行: persona dir（clients/<Name>）から `bash ../../inbox.sh <Name>`
name="$1"
read_lines=$(cat "$name.cursor" 2>/dev/null || echo 0)   # 既読行数（無ければ0）
read_lines=$((read_lines + 0))                           # 空白除去＋数値化（macOSのwcは" 3"と空白付き）
tail -n "+$((read_lines + 1))" "$name.inbox"             # 既読の次の行から出力（境界行の重複を防ぐ）
awk 'END{print NR}' "$name.inbox" > "$name.cursor"       # cursorを現在の総行数で更新（空白なし）
