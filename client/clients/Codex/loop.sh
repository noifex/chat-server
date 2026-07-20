#!/usr/bin/env bash
# loop.sh — Codex の /loop 相当（codex に常駐ループが無いので外側で回す）。
#
# 設計：安価な tick.sh でゲート（notify FIFO 待ちも tick が担当＝busy-spin無し）し、
#       YOUR TURN の時だけ codex を1発呼ぶ＝cold start を実ターンだけに絞る。
#       Claude persona の `/loop 5s` と同じ契約（tick→say）を shell 側で再現。
#
# 実行: clients/Codex から  bash loop.sh
# 停止: turn が STOP になる（Tea の done）／Ctrl-C
set -uo pipefail
cd "$(dirname "$0")"          # clients/Codex に固定（../../ を安定させる）

# codex を headless で1発叩く関数。承認モデルは環境依存なので1箇所に隔離。
# ※ codex の非対話コマンドが `codex exec` でない場合はここだけ直す。
run_codex() {
  # --skip-git-repo-check: codex は git repo外を「非信頼dir」として実行拒否する。ここは
  #   vault非Git方針の workspace 外なので明示スキップ。
  # --sandbox workspace-write: exec のデフォルトは read-only sandbox＝cwd の Codex.outbox
  #   (FIFO) に書けず say.sh が Operation not permitted で死ぬ。cwd 内書き込みを許可する。
  #   ⚠爆風半径は clients/Codex 配下「全体」（outbox だけでなく任意ファイルを書ける）。../../ の
  #   読み(say.sh/protocol.py)は read として許可。書き込みを本当に発言だけに絞りたいなら椅子dir
  #   を空に保つ運用が要る（現状は許容）。
  codex exec --skip-git-repo-check --sandbox workspace-write "あなたは Codex（異種peer・critic）。以下は chat の新着。設計/計画の穴を1つだけ鋭く突き、必ず次のコマンドで送信すること: CHAT_MODEL=codex bash ../../say.sh say \"<批判文> @Coffee\"。新着:
$1"
}

while true; do
  out="$(bash ../../tick.sh Codex)"
  case "$out" in
    STOP*)  echo "[loop] STOP 受信。終了"; break ;;
    SKIP*)  : ;;                              # 自分の番でない。tick が notify で待機済＝spinしない
    "YOUR TURN."*)
      # ヘッダを落とし、自分の残響(Codex:)・join/left を除いた「実質新着」だけ残す
      new="$(printf '%s\n' "$out" | tail -n +2 | grep -vE '^Codex:|joined$|left$' || true)"
      if [ -z "$(printf '%s' "$new" | tr -d '[:space:]')" ]; then
        sleep 2; continue                     # turn は自分だが実質新着なし＝発火せずthrottle（残響での再発火を殺す）
      fi
      run_codex "$new"
      ;;
    *)      echo "[loop] 未知の tick 出力: $out" ;;
  esac
done
