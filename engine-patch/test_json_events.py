"""Tests for the machine-readable JSONL event sink and its CLI wiring."""

from __future__ import annotations

import io
import json
import unittest
from unittest.mock import patch

from typer.testing import CliRunner

from trans_novel.cli import app
from trans_novel.config import Config
from trans_novel.json_events import JsonEventsSink
from trans_novel.llm.providers.fake import FakeClient


class TestJsonEventsSink(unittest.TestCase):
    def _sink(self):
        stream = io.StringIO()
        return JsonEventsSink(stream), stream

    def test_progress_emits_one_json_object_per_line(self):
        sink, stream = self._sink()
        sink(3, 40, "Chapter 3")
        sink(4, 40, "Chapter 3")
        lines = [json.loads(line) for line in stream.getvalue().splitlines()]
        self.assertEqual(len(lines), 2)
        self.assertEqual(
            lines[0], {"event": "progress", "done": 3, "total": 40, "label": "Chapter 3"}
        )
        self.assertEqual(lines[1]["done"], 4)

    def test_indeterminate_label_emits_stage_event(self):
        sink, stream = self._sink()
        sink(0, 0, "Loading review chapters")
        payload = json.loads(stream.getvalue().strip())
        self.assertEqual(payload["event"], "stage")
        self.assertEqual(payload["label"], "Loading review chapters")

    def test_duplicate_stage_and_progress_are_suppressed(self):
        sink, stream = self._sink()
        sink(1, 1, "Restoring checkpoint…")
        sink(1, 1, "Restoring checkpoint…")
        self.assertEqual(len(stream.getvalue().splitlines()), 1)

    def test_done_and_usage_events(self):
        sink, stream = self._sink()
        sink.done(outputs=["a.epub"], chapters_done=40, chapters_total=40)
        sink.usage(
            {
                "usage": {
                    "totals": {
                        "total_tokens": 1234,
                        "prompt_tokens": 100,
                        "completion_tokens": 1134,
                    }
                }
            }
        )
        events = [json.loads(line) for line in stream.getvalue().splitlines()]
        self.assertEqual(events[0]["event"], "done")
        self.assertEqual(events[0]["outputs"], ["a.epub"])
        self.assertEqual(events[1]["event"], "usage")
        self.assertEqual(events[1]["usage"]["totals"]["total_tokens"], 1234)

    def test_usage_skipped_when_empty(self):
        sink, stream = self._sink()
        sink.usage({"usage": {}})
        self.assertEqual(stream.getvalue(), "")

    def test_error_event(self):
        sink, stream = self._sink()
        sink.error("boom")
        self.assertEqual(
            json.loads(stream.getvalue().strip()), {"event": "error", "message": "boom"}
        )

    def test_closed_stream_does_not_raise(self):
        class Closed:
            def write(self, _):
                raise BrokenPipeError

            def flush(self):
                raise BrokenPipeError

        JsonEventsSink(Closed()).error("ignored")  # must not raise


class FakeStore:
    run_dir = "state/book"

    def load_usage(self):
        return None


def _config() -> Config:
    return Config.from_dict({"llm": {"preset": "fake"}})


def _success_result() -> dict:
    return {
        "report": {"summary": {"chapters_done": 2, "chapters_total": 2, "terms": 0}},
        "output": "out.epub",
        "outputs": ["out.epub"],
        "store": FakeStore(),
    }


class _SuccessOrchestrator:
    def __init__(self, config):
        self.client = FakeClient()

    def run_all(self, input_path, *, progress=None, **kwargs):
        if progress is not None:
            progress(0, 0, "Parsing document…")
            progress(1, 2, "Chapter One")
            progress(2, 2, "Chapter Two")
        return _success_result()


class _FailingOrchestrator:
    def __init__(self, config):
        self.client = FakeClient()

    def run_all(self, input_path, *, progress=None, **kwargs):
        raise RuntimeError("model exploded")


class TestCliJsonEvents(unittest.TestCase):
    """The CLI contract: stdout is pure JSONL, human output moves to stderr."""

    def _invoke(self, orchestrator, args):
        with (
            patch("trans_novel.cli._load_config", return_value=_config()),
            patch("trans_novel.pipeline.orchestrator.Orchestrator", orchestrator),
            patch("trans_novel.cli.os.path.isfile", return_value=True),
        ):
            return CliRunner().invoke(app, args)

    @staticmethod
    def _events(result) -> list[dict]:
        return [json.loads(line) for line in result.stdout.splitlines() if line.strip()]

    def test_json_events_streams_stage_progress_and_done(self):
        result = self._invoke(_SuccessOrchestrator, ["--json-events", "translate", "in.txt"])
        self.assertEqual(result.exit_code, 0, result.output)
        events = self._events(result)
        self.assertEqual(events[0], {"event": "stage", "label": "Parsing document…"})
        self.assertEqual(
            events[1], {"event": "progress", "done": 1, "total": 2, "label": "Chapter One"}
        )
        self.assertEqual(events[-1]["event"], "done")
        self.assertEqual(events[-1]["outputs"], ["out.epub"])
        self.assertEqual(events[-1]["chapters_done"], 2)

    def test_stdout_stays_pure_jsonl(self):
        """Every stdout line must parse as JSON; human text belongs on stderr."""
        result = self._invoke(_SuccessOrchestrator, ["--json-events", "translate", "in.txt"])
        for line in result.stdout.splitlines():
            if line.strip():
                json.loads(line)  # raises if framing is broken
        self.assertIn("Complete", result.stderr)

    def test_json_events_terminates_with_error_event_on_failure(self):
        result = self._invoke(_FailingOrchestrator, ["--json-events", "translate", "in.txt"])
        self.assertEqual(result.exit_code, 1)
        events = self._events(result)
        self.assertEqual(events[-1], {"event": "error", "message": "model exploded"})

    def test_default_mode_emits_no_jsonl_on_stdout(self):
        result = self._invoke(_SuccessOrchestrator, ["translate", "in.txt"])
        self.assertEqual(result.exit_code, 0, result.output)
        for line in result.stdout.splitlines():
            self.assertFalse(
                line.strip().startswith('{"event"'),
                f"unexpected JSONL in default mode: {line}",
            )


if __name__ == "__main__":
    unittest.main()
