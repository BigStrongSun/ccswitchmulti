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

tool_payload = json.loads(json.dumps(payload))
tool_payload["input"][0]["content"][0]["text"] = (
    "Call report_marker exactly once with marker CCSM_QWEN38_TOOL_OK. Do not answer with text."
)
tool_request = urllib.request.Request(
    "http://127.0.0.1:15721/v1/responses",
    data=json.dumps(tool_payload).encode("utf-8"),
    headers={
        "Authorization": "Bearer PROXY_MANAGED",
        "Content-Type": "application/json",
        "Accept": "text/event-stream",
        "User-Agent": "Codex Desktop/qwen38-tool-canary",
        "session_id": "qwen38-client-tool-rootfix-20260815",
    },
)
tool_events = []
with urllib.request.urlopen(tool_request, timeout=180) as response:
    for raw_line in response:
        line = raw_line.decode("utf-8").strip()
        if not line.startswith("data:"):
            continue
        data = line[5:].strip()
        if data != "[DONE]":
            tool_events.append(json.loads(data))

tool_serialized = json.dumps(tool_events, ensure_ascii=False)
assert "response.function_call_arguments.done" in [
    event.get("type") for event in tool_events
], tool_serialized[-2000:]
assert "report_marker" in tool_serialized, tool_serialized[-2000:]
assert "CCSM_QWEN38_TOOL_OK" in tool_serialized, tool_serialized[-2000:]
print("CCSM_QWEN38_TOOL_OK")

image_replay_payload = {
    "model": "qwen3.8",
    "stream": True,
    "input": [
        {
            "type": "function_call",
            "call_id": "call_view_image_original",
            "name": "view_image",
            "arguments": json.dumps({"path": "slide-08.png"}),
        },
        {
            "type": "function_call_output",
            "call_id": "call_view_image_original",
            "output": [
                {
                    "type": "input_image",
                    "image_url": (
                        "data:image/png;base64,"
                        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk"
                        "/wcAAgAB/epv2AAAAABJRU5ErkJggg=="
                    ),
                    "detail": "original",
                }
            ],
        },
        {
            "role": "user",
            "content": [
                {
                    "type": "input_text",
                    "text": "Reply only with CCSM_QWEN38_ORIGINAL_REPLAY_OK. Do not call a tool.",
                }
            ],
        },
    ],
}
image_replay_request = urllib.request.Request(
    "http://127.0.0.1:15721/v1/responses",
    data=json.dumps(image_replay_payload).encode("utf-8"),
    headers={
        "Authorization": "Bearer PROXY_MANAGED",
        "Content-Type": "application/json",
        "Accept": "text/event-stream",
        "User-Agent": "Codex Desktop/qwen38-original-replay-canary",
        "session_id": "qwen38-original-replay-rootfix-20260816",
    },
)
image_replay_events = []
with urllib.request.urlopen(image_replay_request, timeout=180) as response:
    assert response.status == 200, response.status
    for raw_line in response:
        line = raw_line.decode("utf-8").strip()
        if not line.startswith("data:"):
            continue
        data = line[5:].strip()
        if data != "[DONE]":
            image_replay_events.append(json.loads(data))

image_replay_serialized = json.dumps(image_replay_events, ensure_ascii=False)
assert "response.completed" in [
    event.get("type") for event in image_replay_events
], image_replay_serialized[-2000:]
assert "CCSM_QWEN38_ORIGINAL_REPLAY_OK" in image_replay_serialized, (
    image_replay_serialized[-2000:]
)
print("CCSM_QWEN38_ORIGINAL_REPLAY_OK")
