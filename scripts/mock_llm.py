"""A minimal OpenAI-compatible mock LLM for end-to-end pipeline verification.

Purpose: exercise the real desktop → sidecar → engine → HTTP → output path without a
paid API key. It answers only the shapes the engine asks for and echoes numbered source
paragraphs back with a marker, so a produced translation is recognisably "translated".

This is a transport stub, not a quality fixture.

Run standalone:
    python scripts/mock_llm.py [port]      # default port 18080
"""

from __future__ import annotations

import json
import re
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

# The engine renders numbered lists as "[0] text" (zero-based, square brackets);
# see trans_novel.agents.prompts.numbered.
NUMBERED_LINE = re.compile(r"^\[(\d+)\]\s?(.*)$")

LOG_LOCK = threading.Lock()
REQUEST_COUNT = {"n": 0}


def numbered_items(text: str) -> list[str]:
    """Extract the numbered source items the engine asked to translate."""
    items: list[tuple[int, str]] = []
    for line in text.splitlines():
        match = NUMBERED_LINE.match(line)
        if match:
            items.append((int(match.group(1)), match.group(2)))
    items.sort(key=lambda pair: pair[0])
    return [content for _, content in items]


def reply_for(body: dict) -> str:
    """Choose a response shape from the operation the engine is performing."""
    system = ""
    user = ""
    for message in body.get("messages") or []:
        role = message.get("role")
        if role == "system":
            system += message.get("content", "")
        elif role == "user":
            user += message.get("content", "")

    blob = (system + "\n" + user).lower()
    items = numbered_items(user)

    if items and "title" in blob:
        return json.dumps({"titles": [f"[T]{t}" for t in items]}, ensure_ascii=False)

    if items and "translat" in blob:
        return json.dumps({"translations": [f"[译]{t}" for t in items]}, ensure_ascii=False)

    # Analysis / synopsis / glossary extraction tolerate an empty object.
    return "{}"


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_args):
        """Silence the default per-request stderr spam."""

    def do_POST(self):  # noqa: N802 - http.server API
        length = int(self.headers.get("Content-Length", "0"))
        raw = self.rfile.read(length) if length else b"{}"
        try:
            body = json.loads(raw.decode("utf-8"))
        except Exception:
            body = {}

        content = reply_for(body)
        with LOG_LOCK:
            REQUEST_COUNT["n"] += 1
            count = REQUEST_COUNT["n"]
        print(f"  [mock] #{count} {self.path} -> {content[:70]}", file=sys.stderr, flush=True)

        payload = {
            "id": f"mock-{count}",
            "object": "chat.completion",
            "created": 0,
            "model": body.get("model", "mock-model"),
            "choices": [
                {
                    "index": 0,
                    "message": {"role": "assistant", "content": content},
                    "finish_reason": "stop",
                }
            ],
            "usage": {"prompt_tokens": 10, "completion_tokens": 10, "total_tokens": 20},
        }
        encoded = json.dumps(payload).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)


def main() -> None:
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 18080
    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    print(f"mock LLM listening on http://127.0.0.1:{port}/v1", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
