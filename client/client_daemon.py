import socket,threading,sys,os,atexit
from protocol import parse_line, render

name=sys.argv[1]
inbox=f"{name}.inbox"
outbox=f"{name}.outbox"
notify= f"{name}.notify"
if not os.path.exists(outbox):
    os.mkfifo(outbox)
if not os.path.exists(notify):
    os.mkfifo(notify)
atexit.register(lambda:os.remove(notify))

s=socket.create_connection(("127.0.0.1",8080))
s.sendall((name +"\n").encode())
atexit.register(lambda:os.remove(outbox))
def recv():
    with open(inbox,"a") as f:
        for line in s.makefile():
            f.write(render(parse_line(line))+"\n")
            f.flush()
threading.Thread(target=recv,daemon=True).start()

while True:
    with open(outbox) as fifo:
        for line in fifo:
            s.sendall(line.encode())