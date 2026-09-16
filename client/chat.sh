#!/usr/bin/env zsh
# chat.sh — chat-server の infra を一発で起こす launcher（案C: AIは各自 claude で手動参加）
#
#   ./chat.sh start        server + 全peer daemon + orchestrator を起動
#   ./chat.sh stop         全部kill + FIFO掃除
#   ./chat.sh status       生存確認
#   ./chat.sh say X "msg"  X の口として発言（テスト用、AIなしで動作確認）
#   ./chat.sh human [name] 人間が render付きの対等 peer として参加（生ncの代わり）
#   ./chat.sh watch X      X.inbox を tail -f（受信をライブ観察）

ROOT="${0:A:h}"                         # このscriptのdir = chat-server-client/
SERVER_DIR="$ROOT/../server"
CLIENTS="$ROOT/clients"
PERSONAS=(Coffee Cola Tea Codex)
CLAUDE_PERSONAS=(Coffee Cola Tea)
SESSION="chat"                          # tmux session 名
RUNTIME="$ROOT/.runtime"
PIDFILE="$RUNTIME/pids"
PY=$(command -v python3 || command -v python)

mkdir -p "$RUNTIME"

abort_recorded_processes() {
  local name pid
  local -a pids
  if [[ -f "$PIDFILE" ]]; then
    while read name pid; do
      kill "$pid" 2>/dev/null
      pids+=("$pid")
    done < "$PIDFILE"
    # Do not let a dying daemon remove a FIFO that a subsequent start adopted.
    for pid in $pids; do
      for _ in {1..20}; do
        kill -0 "$pid" 2>/dev/null || break
        sleep 0.05
      done
      kill -KILL "$pid" 2>/dev/null
    done
    command rm -f "$PIDFILE"
  fi
}

wait_for_daemons() {
  local attempt p pid ready
  for attempt in {1..60}; do
    ready=1
    for p in $PERSONAS; do
      pid=$(awk -v name="$p" '$1 == name { print $2 }' "$PIDFILE")
      [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null || ready=0
      [[ -p "$CLIENTS/$p/$p.outbox" ]] || ready=0
      [[ -p "$CLIENTS/$p/$p.notify" ]] || ready=0
    done
    [[ "$ready" = 1 ]] && return 0
    sleep 0.1
  done
  return 1
}

start() {
  if [[ -f "$PIDFILE" ]]; then
    echo "既に起動中っぽい（$PIDFILE あり）。先に ./chat.sh stop してね"; return 1
  fi
  : > "$PIDFILE"

  echo "▶ server build & 起動..."
  ( cd "$SERVER_DIR" && cargo build ) || {
    echo "✗ build失敗"
    abort_recorded_processes
    return 1
  }
  "$SERVER_DIR/target/debug/chat-server" > "$RUNTIME/server.log" 2>&1 &!
  echo "server $!" >> "$PIDFILE"

  # listening になるまで待つ（初回compile後でも数秒）
  for i in {1..60}; do
    grep -q listening "$RUNTIME/server.log" 2>/dev/null && break
    sleep 0.3
  done
  grep -q listening "$RUNTIME/server.log" 2>/dev/null \
    && echo "  ✓ $(grep listening "$RUNTIME/server.log")" \
    || {
      echo "✗ server が listening にならない"
      cat "$RUNTIME/server.log"
      abort_recorded_processes
      return 1
    }

  echo "▶ daemon 起動..."
  for p in $PERSONAS; do
    ( cd "$CLIENTS/$p" && "$PY" ../../client_daemon.py "$p" ) > "$RUNTIME/$p.log" 2>&1 &!
    echo "$p $!" >> "$PIDFILE"
    echo "  ✓ $p daemon (pid $!)"
  done

  if wait_for_daemons; then
    echo "  ✓ daemon FIFO ready"
  else
    echo "✗ daemon/FIFO の準備に失敗"
    for p in $PERSONAS; do
      [[ -s "$RUNTIME/$p.log" ]] && { echo "--- $p ---"; tail -n 8 "$RUNTIME/$p.log"; }
    done
    abort_recorded_processes
    return 1
  fi

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
  cd $CLIENTS/Codex  && bash loop.sh

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
  for p in $CLAUDE_PERSONAS; do
    ppane[$p]=$(tmux split-window -t "$SESSION" -c "$CLIENTS/$p" -P -F '#{pane_id}')
    tmux send-keys -t "${ppane[$p]}" "claude" C-m
    tmux select-layout -t "$SESSION" tiled
  done

  # Codex is headless: the pane process itself is the loop, so PIDFILE can own it.
  local codex_pane=$(tmux split-window -t "$SESSION" -c "$CLIENTS/Codex" -P -F '#{pane_id}' 'exec bash loop.sh')
  local codex_pid=$(tmux display-message -p -t "$codex_pane" '#{pane_pid}')
  echo "codex-loop $codex_pid" >> "$PIDFILE"
  tmux select-layout -t "$SESSION" tiled

  # 最後のpane: 観察（orchestrator + server のログを tail）
  local obs=$(tmux split-window -t "$SESSION" -c "$ROOT" -P -F '#{pane_id}')
  tmux send-keys -t "$obs" "tail -f '$RUNTIME/orchestrator.log' '$RUNTIME/server.log'" C-m
  tmux select-layout -t "$SESSION" tiled

  # 全claude起動後にまとめて待つ→各paneへ pane-id指定で /loop（Coffeeも取りこぼさない）
  echo "  … claude 起動待ち ${WARMUP}s"
  sleep $WARMUP
  for p in $CLAUDE_PERSONAS; do
    tmux send-keys -t "${ppane[$p]}" "/loop 5s" C-m
  done

  echo "  ✓ human + Coffee/Cola/Tea + Codex + 観察 を1画面に配置。attachする（detach=Ctrl-b d / 停止=./chat.sh stop）"
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
  for p in $PERSONAS; do
    command rm -f "$CLIENTS/$p/$p.outbox" "$CLIENTS/$p/$p.notify" "$CLIENTS/$p/$p.cursor.pending"
  done
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

say()   { ( cd "$CLIENTS/$1" && CHAT_FROM="$1" bash "$ROOT/say.sh" say "$2" ); }
human() { "$PY" "$ROOT/human.py" "${1:-user1}"; }        # ./chat.sh human [name]
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
