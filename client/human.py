#!/usr/bin/env python3
# human.py <name> — 人間が対等な peer として bus に入る REPL クライアント。
#
#   受信: socket の各行を parse_line → render して stdout に綺麗に表示（生JSONを見ない）
#   送信: stdin の各行を build_line で say envelope に包んで送る（人間の平文を JSON peer に昇格）
#
# 生 nc との違い: nc は素の "name: {json}" が見えるだけ。human.py は render を通す＝persona と同じ見え方。
# 実行: chat.sh human [name]   /   python3 human.py user1
import socket, sys, threading
from protocol import build_line, parse_line, render

name = sys.argv[1] if len(sys.argv) > 1 else "human"

s = socket.create_connection(("127.0.0.1", 8080))
s.sendall((name + "\n").encode())          # 1行目=名前（server の read_name）

def recv():
    for raw in s.makefile():               # server が閉じたらループが終わる
        print(render(parse_line(raw)), flush=True)

threading.Thread(target=recv, daemon=True).start()

print(f"[{name} として接続。1行=1発言。Ctrl-D / Ctrl-C で退出]", file=sys.stderr)
try:
    for text in sys.stdin:                  # 人間の入力を1行ずつ
        text = text.rstrip("\n")
        if not text:
            continue
        line = build_line(name, None, "say", text)   # model=None（人間なので）
        s.sendall((line + "\n").encode())
except (KeyboardInterrupt, BrokenPipeError):
    pass
finally:
    s.close()
