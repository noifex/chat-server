import atexit
import signal
import socket
import sys
import threading

from fifo_io import ensure_fifo, unlink_if_same_fifo
from protocol import parse_line, render

if len(sys.argv) != 2:
    raise SystemExit("usage: client_daemon.py <name>")

name = sys.argv[1]
inbox = f"{name}.inbox"
outbox = f"{name}.outbox"
notify = f"{name}.notify"

owned_fifos = {}


def cleanup() -> None:
    for path, identity in owned_fifos.items():
        unlink_if_same_fifo(path, identity)


atexit.register(cleanup)

for fifo_path in (outbox, notify):
    owned_fifos[fifo_path] = ensure_fifo(fifo_path)


def exit_on_signal(signum, _frame) -> None:
    raise SystemExit(128 + signum)


signal.signal(signal.SIGINT, exit_on_signal)
signal.signal(signal.SIGTERM, exit_on_signal)

s = socket.create_connection(("127.0.0.1", 8080))
s.sendall((name + "\n").encode())


def recv() -> None:
    with open(inbox, "a") as inbox_file:
        for line in s.makefile():
            inbox_file.write(render(parse_line(line)) + "\n")
            inbox_file.flush()


threading.Thread(target=recv, daemon=True).start()

while True:
    with open(outbox) as fifo:
        for line in fifo:
            s.sendall(line.encode())
