#!/usr/bin/env python3
"""Validate one Codex turn and hand it to the trusted chat sender."""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from pathlib import Path


FORBIDDEN_HANDOFF = re.compile(r"@(Coffee|Cola|Tea|Codex)\b")
SAY = Path(__file__).resolve().parents[2] / "say.sh"


def load_response(path: Path) -> tuple[str, float]:
    response = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(response, dict) or set(response) != {"text", "confidence"}:
        raise ValueError("response must contain exactly text and confidence")

    text = response["text"]
    confidence = response["confidence"]
    if not isinstance(text, str) or not text.strip() or "\n" in text or "\r" in text:
        raise ValueError("text must be one non-empty physical line")
    if len(text) > 600:
        raise ValueError("text exceeds 600 characters")
    if FORBIDDEN_HANDOFF.search(text):
        raise ValueError("text must not choose the next speaker")
    if isinstance(confidence, bool) or not isinstance(confidence, (int, float)):
        raise ValueError("confidence must be a number")
    if not 0 <= confidence <= 20:
        raise ValueError("confidence must be between 0 and 20")
    return text, float(confidence)


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: send_response.py <response.json>", file=sys.stderr)
        return 2
    try:
        text, confidence = load_response(Path(argv[1]))
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"send_response.py: invalid Codex response: {error}", file=sys.stderr)
        return 1

    environment = os.environ.copy()
    environment.update(
        CHAT_FROM="Codex",
        CHAT_MODEL="codex",
        CHAT_CONF_METHOD="self_report",
        CHAT_CONF_SCALE="0-20",
    )
    result = subprocess.run(
        ["bash", str(SAY), "say", text, "--confidence", f"{confidence:g}"],
        env=environment,
        check=False,
    )
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
