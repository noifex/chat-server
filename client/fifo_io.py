#!/usr/bin/env python3
"""Small FIFO primitives shared by the daemon and shell-facing sender."""

from __future__ import annotations

import os
import select
import stat
import sys


FifoIdentity = tuple[int, int]


class FifoError(RuntimeError):
    """The requested path is not a usable FIFO."""


def _identity(info: os.stat_result) -> FifoIdentity:
    return info.st_dev, info.st_ino


def ensure_fifo(path: str, mode: int = 0o600) -> FifoIdentity:
    """Create path as a FIFO, or adopt it only when it is already a FIFO."""
    try:
        os.mkfifo(path, mode)
    except FileExistsError:
        pass

    info = os.lstat(path)
    if not stat.S_ISFIFO(info.st_mode):
        raise FifoError(f"{path}: exists but is not a FIFO")
    return _identity(info)


def unlink_if_same_fifo(path: str, identity: FifoIdentity) -> None:
    """Remove only the FIFO instance originally adopted by this process."""
    try:
        info = os.lstat(path)
    except FileNotFoundError:
        return

    if stat.S_ISFIFO(info.st_mode) and _identity(info) == identity:
        os.unlink(path)


def write_line(path: str, line: str) -> None:
    """Write one UTF-8 line without ever creating or following the target path."""
    flags = os.O_WRONLY | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        raise FifoError(f"{path}: cannot open FIFO: {error.strerror}") from error

    try:
        info = os.fstat(descriptor)
        if not stat.S_ISFIFO(info.st_mode):
            raise FifoError(f"{path}: is not a FIFO")

        data = (line + "\n").encode("utf-8")
        written = 0
        while written < len(data):
            count = os.write(descriptor, data[written:])
            if count == 0:
                raise BrokenPipeError("FIFO write returned zero bytes")
            written += count
    finally:
        os.close(descriptor)


def wait_for_signal(path: str, timeout_seconds: float) -> bool:
    """Wait for one byte without depending on a platform-specific timeout CLI."""
    flags = os.O_RDWR | os.O_NONBLOCK | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        raise FifoError(f"{path}: cannot open FIFO: {error.strerror}") from error

    try:
        info = os.fstat(descriptor)
        if not stat.S_ISFIFO(info.st_mode):
            raise FifoError(f"{path}: is not a FIFO")
        readable, _, _ = select.select([descriptor], [], [], timeout_seconds)
        if not readable:
            return False
        return bool(os.read(descriptor, 1))
    finally:
        os.close(descriptor)


def main(argv: list[str]) -> int:
    if len(argv) < 2 or argv[1] not in {"send", "wait"}:
        print("usage: fifo_io.py {send <fifo> <line>|wait <fifo> <seconds>}", file=sys.stderr)
        return 2

    try:
        if argv[1] == "send" and len(argv) == 4:
            write_line(argv[2], argv[3])
        elif argv[1] == "wait" and len(argv) == 4:
            wait_for_signal(argv[2], float(argv[3]))
        else:
            print("fifo_io.py: invalid arguments", file=sys.stderr)
            return 2
    except (FifoError, OSError, ValueError) as error:
        print(f"fifo_io.py: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
