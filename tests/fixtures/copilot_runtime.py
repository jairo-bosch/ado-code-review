#!/usr/bin/env python3
"""Minimal stdio protocol fixture; no requests leave this process."""

import json
import os
from pathlib import Path
import sys


def receive():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if not line.strip():
            return json.loads(sys.stdin.buffer.read(int(headers[b"content-length"])))
        key, value = line.split(b":", 1)
        headers[key.lower()] = value.strip()


def send(payload):
    body = json.dumps(payload).encode()
    sys.stdout.buffer.write(f"Content-Length: {len(body)}\r\n\r\n".encode() + body)
    sys.stdout.buffer.flush()


def review(params):
    assert "<!-- ado-code-review:" not in params["prompt"]
    schema = params["responseFormat"]["jsonSchema"]["schema"]
    assert schema["additionalProperties"] is False
    fields = schema["properties"]
    if "effort" in fields:
        reason = " " if Path("blank-triage-reason").exists() else "Synthetic changed logic."
        text = {"effort": "balanced", "reason": reason}
    else:
        assert "Synthetic review" in params["prompt"]
        text = {"overview": "Synthetic review is complete.", "new_findings": [], "existing": []}
        if "Synthetic review" in params["prompt"] and '"new_content":"new\\n"' in params["prompt"]:
            text["new_findings"] = [{
                "title": "Handle empty input", "body": "The input can be empty.",
                "severity": "medium", "location": {"path": "src/lib.rs", "start": 1, "end": 1},
                "suggestion": "check_input();",
            }]
        if "Prior synthetic finding" in params["prompt"]:
            text["existing"] = [{"id": 7, "status": "resolved"}]
            text["new_findings"] = []
    return {
        "messageId": "user-message",
        "aborted": False,
        "response": {
            "id": "assistant-event",
            "timestamp": "2026-01-01T00:00:00Z",
            "type": "assistant.message",
            "data": {"messageId": "assistant-message", "content": json.dumps(text)},
        },
    }


def handle(method, params):
    if method == "connect":
        return {"protocolVersion": 4, "ok": True, "version": "1.0.115"}
    if method == "models.list":
        if Path("reject-catalog").exists():
            raise ValueError("synthetic private service diagnostic")
        names = ["claude-haiku-5.5", "claude-sonnet-5.5"]
        if Path("omit-balanced-model").exists():
            names.pop()
        return {"models": [{"id": name, "name": name, "capabilities": {}} for name in names]}
    if method == "session.create":
        assert params["tools"]["availableTools"] == []
        assert params["model"] in ("claude-haiku-5.5", "claude-sonnet-5.5")
        return {"sessionId": params["sessionId"]}
    if method == "session.options.update":
        assert params["instructions"]["skipCustomInstructions"] is True
        return {"success": True}
    if method == "session.sendAndWait":
        return review(params)
    if method in ("session.detach", "runtime.shutdown"):
        return {}
    raise ValueError(f"unexpected method: {method}")


def main():
    assert os.environ.get("COPILOT_SDK_AUTH_TOKEN") == "synthetic-token"
    assert "AZURE_TOKEN" not in os.environ
    assert os.environ.get("COPILOT_GH_HOST") == "github.example"
    while (request := receive()) is not None:
        if "id" not in request:
            continue
        method = request["method"]
        try:
            result = handle(method, request.get("params") or {})
            reply = {"result": result}
        except (ValueError, KeyError, AssertionError) as error:
            reply = {"error": {"code": -32603, "message": f"{method}: {error}"}}
            with open("fixture-error", "w", encoding="utf-8") as diagnostic:
                print(method, error, sorted((request.get("params") or {}).keys()), file=diagnostic)
        send({"jsonrpc": "2.0", "id": request["id"], **reply})
        if method == "runtime.shutdown":
            break


if __name__ == "__main__":
    main()
