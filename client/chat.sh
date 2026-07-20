#!/usr/bin/env zsh
# chat.sh — chat-server の infra を一発で起こす launcher（案C: AIは各自 claude で手動参加）
#
#   ./chat.sh start        server + Coffee/Cola/Tea daemon を起動
#   ./chat.sh stop         全部kill + FIFO掃除
#   ./chat.sh status       生存確認
#   ./chat.sh say X "msg"  X の口として発言（テスト用、AIなしで動作確認）
#   ./chat.sh human [name] 人間が render付きの対等 peer として参加（生ncの代わり）
#   ./chat.sh watch X      X.inbox を tail -f（受信をライブ観察）

ROOT="${0:A:h}"                         # このscriptのdir = chat-server-client/
SERVER_DIR="$ROOT/../server"
CLIENTS="$ROOT/clients"
PERSONAS=(Coffee Cola Tea)
SESSION="chat"                          # tmux session 名
RUNTIME="$ROOT/.runtime"
PIDFILE="$RUNTIME/pids"
PY=$(command -v python3 || command -v python)

mkdir -p "$RUNTIME"

start() {
  if [[ -f "$PIDFILE" ]]; then
    echo "既に起動中っぽい（$PIDFILE あり）。先に ./chat.sh stop してね"; return 1
  fi
  : > "$PIDFILE"

  echo "▶ server build & 起動..."
  ( cd "$SERVER_DIR" && cargo build ) || { echo "✗ build失敗"; return 1; }
  "$SERVER_DIR/target/debug/chat-server" > "$RUNTIME/server.log" 2>&1 &!
  echo "server $!" >> "$PIDFILE"

  # listening になるまで待つ（初回compile後でも数秒）
  for i in {1..60}; do
    grep -q listening "$RUNTIME/server.log" 2>/dev/null && break
    sleep 0.3
  done
  grep -q listening "$RUNTIME/server.log" 2>/dev/null \
    && echo "  ✓ $(grep listening "$RUNTIME/server.log")" \
    || { echo "✗ server が listening にならない"; cat "$RUNTIME/server.log"; return 1; }

  echo "▶ daemon 起動..."
  for p in $PERSONAS; do
    ( cd "$CLIENTS/$p" && "$PY" ../../client_daemon.py "$p" ) > "$RUNTIME/$p.log" 2>&1 &!
    echo "$p $!" >> "$PIDFILE"
    echo "  ✓ $p daemon (pid $!)"
  done

  echo "▶ orchestrator 起動..."
  echo none > "$CLIENTS/turn"                         # talking stick 初期化（題待ち）
  ( "$PY" "$ROOT/orchestrator.py" ) > "$RUNTIME/orchestrator.log" 2>&1 &!
  echo "orchestrator $!" >> "$PIDFILE"
  echo "  ✓ orchestrator (pid $!)  turn=none"

  cat <<EOF

起動完了。AI参加は各端末で:
  cd $CLIENTS/Coffee && claude   # → /loop で参加ループ
  cd $CLIENTS/Cola   && claude
  cd $CLIENTS/Tea    && claude

人間として喋る / 題を投げる:
  ./chat.sh human user1    （render付き。1行=1発言。Ctrl-D で退出）
  または ./chat.sh say Coffee "テスト発言"

観察: ./chat.sh watch Coffee   停止: ./chat.sh stop
EOF
}

tmuxup() {
  command -v tmux >/dev/null || { echo "✗ tmux が無い。brew install tmux してね"; return 1; }
  if tmux has-session -t "$SESSION" 2>/dev/null; then
    echo "既に tmux session '$SESSION' あり。先に ./chat.sh stop してね"; return 1
  fi

  start || return 1                       # infra（server+daemon+orchestrator）を先に上げる＝server ready保証

  local WARMUP=6                          # claude 起動待ち（初回cold startは遅い。取りこぼしたら増やす）
  typeset -A ppane                        # persona -> pane_id
  echo "▶ tmux '$SESSION' 構築（1ウィンドウに集約）..."

  # pane0: 人間の題投げ（human.py = render 付き対等 peer）。1行=1発言
  local nc_pane=$(tmux new-session -d -s "$SESSION" -c "$ROOT" -P -F '#{pane_id}')
  tmux send-keys -t "$nc_pane" "$PY human.py user1" C-m

  # pane1-3: persona ×3。まず claude を全部起動（並列にwarm up）。/loop は後でまとめて送る
  for p in $PERSONAS; do
    ppane[$p]=$(tmux split-window -t "$SESSION" -c "$CLIENTS/$p" -P -F '#{pane_id}')
    tmux send-keys -t "${ppane[$p]}" "claude" C-m
    tmux select-layout -t "$SESSION" tiled
  done

  # pane4: 観察（orchestrator + server のログを tail）
  local obs=$(tmux split-window -t "$SESSION" -c "$ROOT" -P -F '#{pane_id}')
  tmux send-keys -t "$obs" "tail -f '$RUNTIME/orchestrator.log' '$RUNTIME/server.log'" C-m
  tmux select-layout -t "$SESSION" tiled

  # 全claude起動後にまとめて待つ→各paneへ pane-id指定で /loop（Coffeeも取りこぼさない）
  echo "  … claude 起動待ち ${WARMUP}s"
  sleep $WARMUP
  for p in $PERSONAS; do
    tmux send-keys -t "${ppane[$p]}" "/loop 5s" C-m
  done

  echo "  ✓ nc + Coffee/Cola/Tea + 観察 を1画面に配置。attachする（detach=Ctrl-b d / 停止=./chat.sh stop）"
  tmux attach -t "$SESSION"
}

stop() {
  echo "▶ 停止..."
  tmux kill-session -t "$SESSION" 2>/dev/null && echo "  ✓ tmux session '$SESSION' kill"
  if [[ -f "$PIDFILE" ]]; then
    while read name pid; do
      kill "$pid" 2>/dev/null && echo "  ✓ killed $name ($pid)"
    done < "$PIDFILE"
    command rm -f "$PIDFILE"
  fi
  pkill -f 'target/debug/chat-server' 2>/dev/null   # cargo経由の取りこぼし保険
  pkill -f client_daemon.py 2>/dev/null
  pkill -f orchestrator.py 2>/dev/null              # talking stick の脳も止める
  for p in $PERSONAS; do command rm -f "$CLIENTS/$p/$p.outbox"; done   # FIFO掃除（command でtrash alias回避）
  command rm -f "$CLIENTS/turn"                      # talking stick 掃除
  echo "  ✓ FIFO掃除済み"
}

status() {
  if [[ -f "$PIDFILE" ]]; then
    echo "=== 起動中 ==="; while read name pid; do
      kill -0 "$pid" 2>/dev/null && echo "  ● $name (pid $pid) alive" || echo "  ○ $name (pid $pid) 死亡"
    done < "$PIDFILE"
  else
    echo "停止中（$PIDFILE なし）"
  fi
}

say()   { ( cd "$CLIENTS/$1" && bash "$ROOT/say.sh" say "$2" ); }  # ./chat.sh say Coffee "hi"（JSON envelope）
human() { "$PY" "$ROOT/human.py" "${2:-user1}"; }                  # ./chat.sh human [name]
watch() { tail -f "$CLIENTS/$1/$1.inbox"; }                        # ./chat.sh watch Coffee
watchall() { tail -f "$RUNTIME/chat.log"; }
case "$1" in
  start)  start ;;
  tmux)   tmuxup ;;
  stop)   stop ;;
  status) status ;;
  say)    say "$2" "$3" ;;
  human)  human "$2" ;;
  watch)  watch "$2" ;;
  watch-all) watchall ;;
  *)      echo "usage: $0 {start|tmux|stop|status|say <name> <msg>|human [name]|watch <name>}" ;;
esac
