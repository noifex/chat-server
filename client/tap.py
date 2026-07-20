import socket
# passive observer: serverの全broadcastを生JSONのまま .runtime/raw.log へ。
# 名前を送るだけで以後は受信専用（送信しない＝debateに無害）。
s = socket.create_connection(("127.0.0.1", 8080))
s.sendall(b"tap\n")
with open(".runtime/raw.log", "a") as f:
    for line in s.makefile():
        f.write(line)
        f.flush()
