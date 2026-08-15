import json
import time
import urllib.request


payload = {
    "model": "qwen3.8",
    "stream": True,
    "tool_choice": "auto",
    "input": [
        {
            "role": "user",
            "content": [
                {
                    "type": "input_text",
                    "text": "Reply only with CCSM_QWEN38_STREAM_OK. Do not call a tool.",
                }
            ],
        }
    ],
    "tools": [
        {"type": "web_search"},
        {
            "type": "function",
            "name": "report_marker",
            "description": "Report a marker only when explicitly requested.",
            "parameters": {
                "type": "object",
                "properties": {"marker": {"type": "string"}},
                "required": ["marker"],
                "additionalProperties": False,
            },
        },
    ],
}
request = urllib.request.Request(
    "http://127.0.0.1:15721/v1/responses",
    data=json.dumps(payload).encode("utf-8"),
    headers={
        "Authorization": "Bearer PROXY_MANAGED",
        "Content-Type": "application/json",
        "Accept": "text/event-stream",
        "User-Agent": "Codex Desktop/qwen38-stream-canary",
        "session_id": "qwen38-stream-rootfix-20260815",
    },
)

started = time.monotonic()
events = []
with urllib.request.urlopen(request, timeout=180) as response:
    assert response.headers.get_content_type() == "text/event-stream", response.headers
    first_event_seconds = None
    for raw_line in response:
        line = raw_line.decode("utf-8").strip()
        if not line.startswith("data:"):
            continue
        if first_event_seconds is None:
            first_event_seconds = time.monotonic() - started
        data = line[5:].strip()
        if data != "[DONE]":
            events.append(json.loads(data))

event_types = [event.get("type") for event in events]
assert "response.completed" in event_types, event_types
serialized = json.dumps(events, ensure_ascii=False)
assert "CCSM_QWEN38_STREAM_OK" in serialized, serialized[-2000:]
print(
    json.dumps(
        {
            "status": "CCSM_QWEN38_STREAM_OK",
            "first_event_seconds": round(first_event_seconds or 0.0, 3),
            "event_count": len(events),
            "event_types": event_types,
        },
        ensure_ascii=False,
    )
)
