"""Run a real third-party Codex hosted web_search canary through CCSM.

The local proxy owns provider credentials. This script only sends the
PROXY_MANAGED marker and never reads or prints stored secrets.
"""

from __future__ import annotations

import json
import os
import sys
import time
import urllib.request
import uuid


ENDPOINT = "http://127.0.0.1:15721/v1/responses"
MODEL = os.environ.get("CCSM_CANARY_MODEL", "qwen3.8")
MARKER = "CCSM_THIRD_PARTY_HOSTED_SEARCH_OK"


def read_sse(response: urllib.response.addinfourl) -> list[dict]:
    events: list[dict] = []
    for raw_line in response:
        line = raw_line.decode("utf-8").strip()
        if not line.startswith("data:"):
            continue
        data = line[5:].strip()
        if data != "[DONE]":
            events.append(json.loads(data))
    return events


def main() -> int:
    session_id = f"third-party-hosted-search-{uuid.uuid4()}"
    payload = {
        "model": MODEL,
        "stream": True,
        "tool_choice": {"type": "web_search"},
        "input": [
            {
                "role": "user",
                "content": [
                    {
                        "type": "input_text",
                        "text": (
                            "Use the web_search tool exactly once to search for "
                            "'OpenAI Codex web search'. Then reply only with "
                            f"{MARKER}."
                        ),
                    }
                ],
            }
        ],
        "tools": [{"type": "web_search"}],
    }
    request = urllib.request.Request(
        ENDPOINT,
        data=json.dumps(payload).encode("utf-8"),
        headers={
            "Authorization": "Bearer PROXY_MANAGED",
            "Content-Type": "application/json",
            "Accept": "text/event-stream",
            "User-Agent": "Codex Desktop/third-party-hosted-search-canary",
            "session_id": session_id,
            "x-codex-turn-metadata": "ccsm-third-party-hosted-search-canary",
        },
    )
    started = time.monotonic()
    try:
        with urllib.request.urlopen(request, timeout=240) as response:
            events = read_sse(response)
            header = response.headers.get("x-cc-switch-hosted-tool-stream-response")
            status = response.status
    except Exception as error:
        print(json.dumps({"status": "error", "error": str(error)}))
        return 2

    event_types = [event.get("type") for event in events]
    serialized = json.dumps(events, ensure_ascii=False)
    result = {
        "status": "ok" if status == 200 and MARKER in serialized else "failed",
        "http_status": status,
        "model": MODEL,
        "session_id": session_id,
        "hosted_stream_header": header,
        "event_count": len(events),
        "event_types": event_types,
        "elapsed_seconds": round(time.monotonic() - started, 3),
        "marker_present": MARKER in serialized,
        "text": "".join(
            str(event.get("delta", ""))
            for event in events
            if event.get("type") == "response.output_text.delta"
        )[-1000:],
    }
    print(json.dumps(result, ensure_ascii=False))
    return 0 if result["status"] == "ok" else 1


if __name__ == "__main__":
    sys.exit(main())
