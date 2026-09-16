#!/usr/bin/env python3
"""Run one command with a portable timeout and process-group cleanup."""

from __future__ import annotations

import os
import signal
import subprocess
import sys


def main(argv: list[str]) -> int:
    if len(argv) < 3:
        print(
            "usage: run_with_timeout.py <seconds> [--stdin <path>] <command> [args...]",
            file=sys.stderr,
        )
        return 2

    try:
        timeout_seconds = float(argv[1])
    except ValueError:
        print(f"run_with_timeout.py: invalid timeout: {argv[1]}", file=sys.stderr)
        return 2
    if timeout_seconds <= 0:
        print("run_with_timeout.py: timeout must be positive", file=sys.stderr)
        return 2

    command_index = 2
    input_file = None
    if argv[command_index] == "--stdin":
        if len(argv) < 5:
            print("run_with_timeout.py: --stdin requires a path and command", file=sys.stderr)
            return 2
        input_file = open(argv[command_index + 1], "rb")
        command_index += 2

    try:
        process = subprocess.Popen(
            argv[command_index:],
            stdin=input_file if input_file is not None else subprocess.DEVNULL,
            start_new_session=True,
        )

        def terminate_child_group(signum, _frame):
            if process.poll() is None:
                try:
                    os.killpg(process.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    try:
                        os.killpg(process.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    process.wait()
            raise SystemExit(128 + signum)

        signal.signal(signal.SIGINT, terminate_child_group)
        signal.signal(signal.SIGTERM, terminate_child_group)
        try:
            return process.wait(timeout=timeout_seconds)
        except subprocess.TimeoutExpired:
            print(
                f"run_with_timeout.py: timed out after {timeout_seconds:g}s: {argv[command_index]}",
                file=sys.stderr,
            )
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                process.wait()
            return 124
    finally:
        if input_file is not None:
            input_file.close()


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
