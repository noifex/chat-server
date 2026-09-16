#!/usr/bin/env bash
# inbox.sh <Name> — 新着を出力し、say.sh が確定する pending cursor を作る。
# 目的: command substitution $() を script 内に封じ込め、persona が打つコマンドを
#       静的にする（→ Claude Code の allowlist で事前許可でき、毎回の承認が消える）。
# 実行: persona dir（clients/<Name>）から `bash ../../inbox.sh <Name>`
name="$1"
read_lines=$(cat "$name.cursor" 2>/dev/null || echo 0)
case "$read_lines" in
    ''|*[!0-9]*) echo "inbox.sh: invalid cursor: $read_lines" >&2; exit 1;;
esac

total=$(awk 'END{print NR + 0}' "$name.inbox")
[ "$read_lines" -le "$total" ] || {
    echo "inbox.sh: cursor $read_lines exceeds inbox length $total" >&2
    exit 1
}

pending="$name.cursor.pending"
pending_tmp=$(mktemp "${pending}.tmp.XXXXXX") || exit 1
trap 'rm -f "$pending_tmp"' EXIT
printf '%s\n' "$total" > "$pending_tmp"
mv "$pending_tmp" "$pending"
trap - EXIT

if [ "$read_lines" -lt "$total" ]; then
    sed -n "$((read_lines + 1)),${total}p" "$name.inbox"
fi
