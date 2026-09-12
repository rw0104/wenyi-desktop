"""Machine-readable JSONL event reporting for desktop/automation clients.

The CLI renders a Rich progress bar by default. Wenyi Desktop (a Tauri shell) drives
the same engine as a subprocess and needs a structured transport instead of terminal
escape sequences. ``--json-events`` switches the progress callback to emit one JSON
object per line on stdout:

    {"event": "progress", "done": 3, "total": 40, "label": "Chapter 3"}
    {"event": "done", "outputs": ["..."], "chapters_done": 40, "chapters_total": 40}

Events are self-contained and line-delimited so a client can parse them incrementally,
resume after a partial read, and never depend on ANSI control sequences.

This module is transport-only. It imports no pipeline/services and must stay free of
domain or state-machine knowledge; leave the CLI to map stage results onto events.
"""

from __future__ import annotations

import json
import sys
from typing import Any, TextIO


class JsonEventsSink:
    """Write one JSON object per line to a stream; ``__call__`` matches ``ProgressFn``."""

    def __init__(self, stream: TextIO | None = None) -> None:
        self._stream = stream if stream is not None else sys.stdout
        # Last determinate (label, total, done) and indeterminate (label, None) to
        # suppress only true no-op events while letting counts advance.
        self._last_progress: tuple[str, int, int] | None = None
        self._last_stage: tuple[str, None] | None = None

    def emit(self, event: str, **fields: Any) -> None:
        """Emit a single JSONL event object atomically.

        A closed consumer (desktop client exited) must not abort translation, which is
        resumable; drop the event and keep the engine running.
        """
        payload: dict[str, Any] = {"event": event}
        payload.update(fields)
        line = json.dumps(payload, ensure_ascii=False, separators=(",", ":"))
        try:
            self._stream.write(line + "\n")
            self._stream.flush()
        except (BrokenPipeError, ValueError, OSError):
            pass

    def __call__(self, done: int, total: int, label: str) -> None:
        """Report stage progress; mirrors the Rich bridge's ``ProgressFn`` contract."""
        if total > 0:
            key = (label, total, done)
            if key == self._last_progress:
                return
            self._last_progress = key
            self.emit("progress", done=done, total=total, label=label)
        else:
            key = (label, None)
            if key == self._last_stage:
                return
            self._last_stage = key
            self.emit("stage", label=label)

    def usage(self, report: dict[str, Any]) -> None:
        """Emit cumulative token usage, mirroring the CLI's ``_print_usage`` input."""
        usage = report.get("usage") or {}
        if not usage.get("totals", {}).get("total_tokens"):
            return
        self.emit("usage", usage=usage)

    def done(self, *, outputs: list[str], chapters_done: int, chapters_total: int) -> None:
        """Emit a terminal ``done`` event after a successful run."""
        self.emit(
            "done",
            outputs=outputs,
            chapters_done=chapters_done,
            chapters_total=chapters_total,
        )

    def error(self, message: str) -> None:
        """Emit a terminal ``error`` event."""
        self.emit("error", message=message)
