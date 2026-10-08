#!/usr/bin/env python3
"""Deterministic local Agent Client Protocol fixture for native QA."""

import json
import sys
import time
from urllib.parse import unquote, urlparse


SESSION_ID = "mock-session"
next_id = 1


def send(message):
    sys.stdout.write(json.dumps(message, ensure_ascii=False, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def response(message_id, result):
    send({"jsonrpc": "2.0", "id": message_id, "result": result})


def error(message_id, code, text):
    send({"jsonrpc": "2.0", "id": message_id, "error": {"code": code, "message": text}})


def request(method, params):
    global next_id
    message_id = next_id
    next_id += 1
    send({"jsonrpc": "2.0", "id": message_id, "method": method, "params": params})
    return message_id


def update(value):
    send(
        {
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {"sessionId": SESSION_ID, "update": value},
        }
    )


def wait_for_reply(message_id, prompt_id):
    for line in sys.stdin:
        try:
            incoming = json.loads(line)
        except json.JSONDecodeError:
            continue
        if incoming.get("method") == "session/cancel":
            response(prompt_id, {"stopReason": "cancelled"})
            return None
        if incoming.get("id") == message_id:
            return incoming
    response(prompt_id, {"stopReason": "cancelled"})
    return None


def prompt(message_id, params):
    resource_path = None
    for block in params.get("prompt", []):
        if block.get("type") in ("resource_link", "resource"):
            resource = block.get("resource", block)
            uri = resource.get("uri", "")
            parsed = urlparse(uri)
            if parsed.scheme == "file":
                resource_path = unquote(parsed.path)
                break
    if resource_path is None:
        response(message_id, {"stopReason": "end_turn"})
        return

    update({"sessionUpdate": "plan", "entries": [{"content": "Read and propose a small edit", "status": "in_progress"}]})
    for chunk in ("Mock agent connected. ", "I will inspect the selected note, ", "then propose a reviewable edit."):
        update(
            {
                "sessionUpdate": "agent_message_chunk",
                "messageId": "mock-message",
                "content": {"type": "text", "text": chunk},
            }
        )
        time.sleep(0.04)

    tool_id = "mock-read-write"
    update(
        {
            "sessionUpdate": "tool_call",
            "toolCallId": tool_id,
            "title": "Read selected note",
            "status": "in_progress",
        }
    )
    permission_id = request(
        "session/request_permission",
        {
            "sessionId": SESSION_ID,
            "toolCall": {"toolCallId": tool_id},
            "options": [
                {"optionId": "allow-once", "name": "Allow once", "kind": "allow_once"},
                {"optionId": "reject-once", "name": "Reject", "kind": "reject_once"},
            ],
        },
    )
    permission = wait_for_reply(permission_id, message_id)
    if permission is None:
        return
    outcome = permission.get("result", {}).get("outcome", {})
    if outcome.get("outcome") != "selected" or outcome.get("optionId") != "allow-once":
        update(
            {
                "sessionUpdate": "tool_call_update",
                "toolCallId": tool_id,
                "status": "failed",
                "content": [{"type": "content", "content": {"type": "text", "text": "Permission declined."}}],
            }
        )
        update(
            {
                "sessionUpdate": "agent_message_chunk",
                "messageId": "mock-message",
                "content": {"type": "text", "text": "\nPermission was declined; no file was changed."},
            }
        )
        response(message_id, {"stopReason": "end_turn"})
        return

    read_id = request(
        "fs/read_text_file",
        {"sessionId": SESSION_ID, "path": resource_path, "line": 1, "limit": 200},
    )
    read_reply = wait_for_reply(read_id, message_id)
    if read_reply is None:
        return
    if "error" in read_reply:
        update(
            {
                "sessionUpdate": "agent_message_chunk",
                "messageId": "mock-message",
                "content": {"type": "text", "text": f"\nFile read failed: {read_reply['error'].get('message', 'error')}"},
            }
        )
        response(message_id, {"stopReason": "end_turn"})
        return
    old_text = read_reply.get("result", {}).get("content", "")
    new_text = old_text + ("" if not old_text or old_text.endswith("\n") else "\n") + "Mock agent edit.\n"
    update(
        {
            "sessionUpdate": "tool_call_update",
            "toolCallId": tool_id,
            "status": "completed",
            "content": [
                {
                    "type": "diff",
                    "diff": {"path": resource_path, "oldText": old_text, "newText": new_text},
                }
            ],
        }
    )
    write_id = request(
        "fs/write_text_file",
        {"sessionId": SESSION_ID, "path": resource_path, "content": new_text},
    )
    write_reply = wait_for_reply(write_id, message_id)
    if write_reply is None:
        return
    if "error" in write_reply:
        final_text = "\nThe proposed change was rejected; the file was not changed."
    else:
        final_text = "\nThe proposed change was accepted."
    update(
        {
            "sessionUpdate": "agent_message_chunk",
            "messageId": "mock-message",
            "content": {"type": "text", "text": final_text},
        }
    )
    response(message_id, {"stopReason": "end_turn"})


for line in sys.stdin:
    try:
        message = json.loads(line)
    except json.JSONDecodeError:
        continue
    method = message.get("method")
    message_id = message.get("id")
    params = message.get("params", {})
    if method == "initialize":
        response(
            message_id,
            {
                "protocolVersion": 1,
                "agentInfo": {"name": "mock-acp-agent", "version": "1.0"},
                "agentCapabilities": {},
                "authMethods": [],
            },
        )
    elif method == "session/new":
        response(message_id, {"sessionId": SESSION_ID, "modes": {"currentModeId": "default", "availableModes": []}})
    elif method == "session/prompt":
        prompt(message_id, params)
    elif method == "session/cancel":
        continue
    elif method and message_id is not None:
        error(message_id, -32601, f"Unsupported method: {method}")
