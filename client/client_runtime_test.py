import json
import os
import stat
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

from fifo_io import (
    FifoError,
    ensure_fifo,
    unlink_if_same_fifo,
    wait_for_signal,
    write_line,
)


ROOT = Path(__file__).resolve().parent
SAY = ROOT / "say.sh"
TICK = ROOT / "tick.sh"
TIMEOUT_RUNNER = ROOT / "run_with_timeout.py"
CODEX_SEND = ROOT / "clients" / "Codex" / "send_response.py"
CODEX_LOOP = ROOT / "clients" / "Codex" / "loop.sh"


def run_say(directory: Path, name: str, text: str) -> subprocess.CompletedProcess:
    environment = os.environ.copy()
    environment.update(CHAT_FROM=name, CHAT_MODEL="runtime-test")
    return subprocess.run(
        ["bash", str(SAY), "say", text],
        cwd=directory,
        env=environment,
        capture_output=True,
        text=True,
        check=False,
    )


with tempfile.TemporaryDirectory() as temporary:
    root = Path(temporary)
    persona = root / "Tester"
    persona.mkdir()

    # FIFO ownership is inode-specific, so an old process cannot delete a replacement.
    fifo = persona / "owned"
    identity = ensure_fifo(str(fifo))
    assert stat.S_ISFIFO(fifo.stat().st_mode)
    unlink_if_same_fifo(str(fifo), (-1, -1))
    assert fifo.exists()
    unlink_if_same_fifo(str(fifo), identity)
    assert not fifo.exists()

    notify = persona / "notify"
    ensure_fifo(str(notify))
    signaled = []

    def wait_once() -> None:
        signaled.append(wait_for_signal(str(notify), 1))

    waiter = threading.Thread(target=wait_once, daemon=True)
    waiter.start()
    time.sleep(0.02)
    write_line(str(notify), "1")
    waiter.join(timeout=2)
    assert signaled == [True], signaled
    assert wait_for_signal(str(notify), 0.01) is False

    regular = persona / "regular"
    regular.write_text("sentinel", encoding="utf-8")
    try:
        write_line(str(regular), "must not be written")
        raise AssertionError("regular file was accepted as a FIFO")
    except FifoError:
        pass
    assert regular.read_text(encoding="utf-8") == "sentinel"

    # A missing outbox must fail without accidentally creating a regular file.
    result = run_say(persona, "Tester", "missing")
    assert result.returncode != 0, result
    assert not (persona / "Tester.outbox").exists()

    # tick peeks a stable snapshot.  It does not commit the cursor itself.
    (root / "turn").write_text("Tester", encoding="utf-8")
    (persona / "Tester.inbox").write_text("old\nnew-1\nnew-2\n", encoding="utf-8")
    (persona / "Tester.cursor").write_text("1\n", encoding="utf-8")
    result = subprocess.run(
        ["bash", str(TICK), "Tester"],
        cwd=persona,
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, result
    assert result.stdout == "YOUR TURN. new:\nnew-1\nnew-2\n", result.stdout
    assert (persona / "Tester.cursor").read_text(encoding="utf-8") == "1\n"
    assert (persona / "Tester.cursor.pending").read_text(encoding="utf-8") == "3\n"

    # A failed send keeps pending work unacknowledged and leaves a regular file untouched.
    outbox = persona / "Tester.outbox"
    outbox.write_text("sentinel", encoding="utf-8")
    result = run_say(persona, "Tester", "blocked")
    assert result.returncode != 0, result
    assert outbox.read_text(encoding="utf-8") == "sentinel"
    assert (persona / "Tester.cursor").read_text(encoding="utf-8") == "1\n"
    assert (persona / "Tester.cursor.pending").exists()
    outbox.unlink()

    # A successful FIFO handoff atomically commits only the displayed snapshot.
    os.mkfifo(outbox)
    received = []

    def read_once() -> None:
        with outbox.open(encoding="utf-8") as stream:
            received.append(stream.readline())

    reader = threading.Thread(target=read_once, daemon=True)
    reader.start()
    result = run_say(persona, "Tester", "delivered")
    reader.join(timeout=2)
    assert not reader.is_alive()
    assert result.returncode == 0, result
    event = json.loads(received[0])
    assert event["from"] == "Tester" and event["text"] == "delivered", event
    assert (persona / "Tester.cursor").read_text(encoding="utf-8") == "3\n"
    assert not (persona / "Tester.cursor.pending").exists()

    # The portable timeout kills a stuck process and preserves stdin when requested.
    input_path = root / "input"
    input_path.write_text("from-file", encoding="utf-8")
    result = subprocess.run(
        [
            sys.executable,
            str(TIMEOUT_RUNNER),
            "1",
            "--stdin",
            str(input_path),
            sys.executable,
            "-c",
            "import sys; assert sys.stdin.read() == 'from-file'",
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, result
    result = subprocess.run(
        [
            sys.executable,
            str(TIMEOUT_RUNNER),
            "0.05",
            sys.executable,
            "-c",
            "import time; time.sleep(2)",
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 124, result

    # Only the trusted Codex wrapper may turn structured model output into a send.
    codex = root / "Codex"
    codex.mkdir()
    invalid_response = codex / "invalid.json"
    invalid_response.write_text(
        json.dumps({"text": "bad @Coffee handoff", "confidence": 20}),
        encoding="utf-8",
    )
    result = subprocess.run(
        [sys.executable, str(CODEX_SEND), str(invalid_response)],
        cwd=codex,
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode != 0, result
    assert not (codex / "Codex.outbox").exists()

    codex_outbox = codex / "Codex.outbox"
    os.mkfifo(codex_outbox)
    (codex / "Codex.cursor.pending").write_text("7\n", encoding="utf-8")
    response = codex / "response.json"
    response.write_text(
        json.dumps({"text": "境界条件が未定義です。", "confidence": 13}),
        encoding="utf-8",
    )
    codex_received = []

    def read_codex_once() -> None:
        with codex_outbox.open(encoding="utf-8") as stream:
            codex_received.append(stream.readline())

    reader = threading.Thread(target=read_codex_once, daemon=True)
    reader.start()
    result = subprocess.run(
        [sys.executable, str(CODEX_SEND), str(response)],
        cwd=codex,
        capture_output=True,
        text=True,
        check=False,
    )
    reader.join(timeout=2)
    assert result.returncode == 0 and not reader.is_alive(), result
    event = json.loads(codex_received[0])
    assert event["from"] == "Codex" and event["model"] == "codex", event
    assert event["confidence"] == 13 and event["confidence_method"] == "self_report", event
    assert event["confidence_scale"] == "0-20", event
    assert (codex / "Codex.cursor").read_text(encoding="utf-8") == "7\n"

    # Exercise the complete Codex loop with a fake CLI, isolated from real inbox state.
    loop_root = root / "loop"
    loop_peer = loop_root / "Codex"
    loop_peer.mkdir(parents=True)
    (loop_root / "turn").write_text("Codex", encoding="utf-8")
    (loop_peer / "Codex.inbox").write_text("Coffee: proposal\n", encoding="utf-8")
    (loop_peer / "Codex.cursor").write_text("0\n", encoding="utf-8")
    loop_outbox = loop_peer / "Codex.outbox"
    os.mkfifo(loop_outbox)

    fake_bin = root / "bin"
    fake_bin.mkdir()
    fake_codex = fake_bin / "codex"
    fake_codex.write_text(
        """#!/usr/bin/env python3
import json
import sys

arguments = sys.argv[1:]
output = arguments[arguments.index("--output-last-message") + 1]
sys.stdin.read()
with open(output, "w", encoding="utf-8") as stream:
    json.dump({"text": "監査応答です。", "confidence": 11}, stream, ensure_ascii=False)
""",
        encoding="utf-8",
    )
    fake_codex.chmod(0o755)

    loop_received = []

    def read_loop_once() -> None:
        with loop_outbox.open(encoding="utf-8") as stream:
            loop_received.append(stream.readline())

    reader = threading.Thread(target=read_loop_once, daemon=True)
    reader.start()
    environment = os.environ.copy()
    environment.update(
        CODEX_PEER_DIR=str(loop_peer),
        CODEX_LOOP_ONCE="1",
        CODEX_TIMEOUT_SECONDS="2",
        PATH=str(fake_bin) + os.pathsep + environment["PATH"],
    )
    result = subprocess.run(
        ["bash", str(CODEX_LOOP)],
        env=environment,
        capture_output=True,
        text=True,
        check=False,
    )
    reader.join(timeout=2)
    assert result.returncode == 0 and not reader.is_alive(), result
    event = json.loads(loop_received[0])
    assert event["from"] == "Codex" and event["text"] == "監査応答です。", event
    assert event["confidence"] == 11, event
    assert (loop_peer / "Codex.cursor").read_text(encoding="utf-8") == "1\n"

print("ok")
