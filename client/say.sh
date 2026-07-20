#!/usr/bin/env bash
# say.sh <type> "<text>" [--reply N] [--task N] [--confidence C]
#
# JSON行を1本組んで <CHAT_FROM>.outbox に書く（daemon が sendall で server へ）。
# JSON の組立・エスケープは protocol.build_line に丸投げ＝bash は引数を渡す皮だけ。
# 実行: persona dir（clients/<Name>）から
#     bash ../../say.sh say  "本文 @Tea"
#     bash ../../say.sh done "収束した"        # 終端は type=done（text と混ざらない）
#
# from は cwd の dir 名から自動導出（clients/Cola で実行 → Cola）。
#   → persona 側は env を打たなくていい＝コマンドが `bash ../../say.sh ...` のまま
#     ＝allowlist `Bash(bash ../../say.sh:*)` が効く＝承認ゼロ維持。
#   どちらも env で上書き可。
set -euo pipefail
CHAT_FROM="${CHAT_FROM:-$(basename "$PWD")}"
CHAT_MODEL="${CHAT_MODEL:-claude-sonnet-5}"   # persona=sonnet（2026-07-17〜）。異種は codex 等が別値、軸4で per-persona 差し替え
# 設計デルタ①: confidence の出所と domain は env default 可（毎回打たせない＝from/model と同じ規律）
CHAT_CONF_METHOD="${CHAT_CONF_METHOD:-}"      # self_report | logprob | sampled
CHAT_CONF_SCALE="${CHAT_CONF_SCALE:-}"        # 例 0-20
CHAT_DOMAIN="${CHAT_DOMAIN:-}"                # 例 rust

if [ "$#" -lt 2 ]; then
  echo "usage: say.sh <type> \"<text>\" [--reply N] [--task N] [--confidence C] [--confidence-method M] [--confidence-scale S] [--domain D]" >&2
  exit 1
fi

type="$1"; text="$2"; shift 2
reply=""; task=""; conf=""
cmethod="$CHAT_CONF_METHOD"; cscale="$CHAT_CONF_SCALE"; domain="$CHAT_DOMAIN"
while [ "$#" -gt 0 ]; do
  case "$1" in
    --reply)             reply="$2";   shift 2;;
    --task)              task="$2";    shift 2;;
    --confidence)        conf="$2";    shift 2;;
    --confidence-method) cmethod="$2"; shift 2;;
    --confidence-scale)  cscale="$2";  shift 2;;
    --domain)            domain="$2";  shift 2;;
    *) echo "say.sh: unknown arg $1" >&2; exit 1;;
  esac
done

# protocol.py は say.sh と同階層（chat-server-client/ の root）に置く前提。
# -c は cwd が persona dir なので、明示的に PYTHONPATH を通す。
here="$(cd "$(dirname "$0")" && pwd)"

line="$(PYTHONPATH="$here" CHAT_FROM="$CHAT_FROM" CHAT_MODEL="$CHAT_MODEL" python3 -c '
import sys, os, protocol
reply = int(sys.argv[3]) if sys.argv[3] else None
task  = int(sys.argv[4]) if sys.argv[4] else None
conf  = float(sys.argv[5]) if sys.argv[5] else None
print(protocol.build_line(os.environ["CHAT_FROM"], os.environ["CHAT_MODEL"],
                          sys.argv[1], sys.argv[2], reply, task, conf,
                          confidence_method=sys.argv[6] or None,
                          confidence_scale=sys.argv[7] or None,
                          domain=sys.argv[8] or None))
' "$type" "$text" "$reply" "$task" "$conf" "$cmethod" "$cscale" "$domain")"

printf '%s\n' "$line" > "$CHAT_FROM.outbox"
