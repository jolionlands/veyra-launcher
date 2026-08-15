#!/usr/bin/env python3
"""Veyra JSON-RPC plugin bridge for Hippo (the memory/graph store that Silt feeds).

Requires a running Hippo HTTP server (e.g. `hippo serve --dir <store> --port 7345`).
Add to your Veyra `plugins.toml`:

[[plugins]]
id = "hippo"
label = "Hippo Memory"
kind = "json_rpc_stdio"
command = "python"
args = ["C:\\Path\\To\\hippo-veyra-bridge.py"]
keywords = ["hippo", "memory", "recall", "notes"]
enabled = true
"""

import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request

DEFAULT_HOST = os.environ.get("HIPPO_HOST", "127.0.0.1:7345")


def hippo_url(path: str) -> str:
    return f"http://{DEFAULT_HOST}{path}"


def http_get(path: str) -> tuple[int, str]:
    try:
        with urllib.request.urlopen(hippo_url(path), timeout=2.0) as resp:
            return resp.status, resp.read().decode("utf-8", errors="replace")
    except urllib.error.HTTPError as exc:
        return exc.code, exc.read().decode("utf-8", errors="replace")
    except Exception as exc:
        return 0, str(exc)


def send_response(request_id, result=None, error=None):
    response = {"jsonrpc": "2.0", "id": request_id}
    if error is not None:
        response["error"] = error
    else:
        response["result"] = result
    print(json.dumps(response), flush=True)


def static_catalog_items():
    return {
        "items": [
            {
                "id": "hippo.recall",
                "label": "Hippo: Recall from memory",
                "subtitle": "Search the Hippo graph store",
                "category": "tool",
                "keywords": ["hippo", "memory", "recall", "search"],
                "actions": [
                    {
                        "id": "default",
                        "label": "Recall",
                        "kind": "tool_call",
                        "requires_confirmation": False,
                    }
                ],
            },
            {
                "id": "hippo.timeline",
                "label": "Hippo: Recent memory",
                "subtitle": "Show recent nodes and edges",
                "category": "tool",
                "keywords": ["hippo", "memory", "recent", "timeline"],
                "actions": [
                    {
                        "id": "default",
                        "label": "Timeline",
                        "kind": "tool_call",
                        "requires_confirmation": False,
                    }
                ],
            },
        ]
    }


def search_memory(query: str):
    """Search Hippo /api/nodes by topic substring, fallback to /api/recall with fts."""
    encoded = urllib.parse.quote(query)
    status, raw = http_get(f"/api/nodes?topic_substring={encoded}&limit=10")
    if status == 200:
        try:
            data = json.loads(raw)
            nodes = data.get("nodes", [])
        except Exception:
            nodes = []
    else:
        nodes = []

    # If topic substring returns nothing, try full-text recall (no embeddings required).
    if not nodes:
        status, raw = http_get(f"/api/recall?query={encoded}&k=5&fts=1")
        if status == 200:
            try:
                data = json.loads(raw)
                nodes = [hit.get("node", hit) for hit in data.get("results", [])]
            except Exception:
                nodes = []

    items = []
    for node in nodes[:10]:
        topic = node.get("topic", "")
        content = node.get("content", "") or node.get("text", "")
        kind = node.get("kind", "memory")
        node_id = node.get("id", "")
        if not topic:
            continue
        label = topic
        subtitle = f"[{kind}] {content[:120]}" if content else f"[{kind}]"
        items.append({
            "id": f"hippo.node:{node_id}",
            "label": label,
            "subtitle": subtitle,
            "category": "tool",
            "keywords": ["hippo", kind, topic],
            "actions": [
                {
                    "id": "open",
                    "label": "Copy content",
                    "kind": "tool_call",
                    "command": content or topic,
                    "requires_confirmation": False,
                }
            ],
        })

    return {"items": items}


def timeline_items():
    status, raw = http_get("/api/timeline?limit=10")
    if status != 200:
        return {"items": []}
    try:
        data = json.loads(raw)
        events = data.get("events", [])
    except Exception:
        events = []

    items = []
    for event in events[:10]:
        topic = event.get("topic", "")
        kind = event.get("kind", "memory")
        if not topic:
            continue
        items.append({
            "id": f"hippo.timeline:{event.get('id', '')}",
            "label": topic,
            "subtitle": f"[{kind}] recent memory",
            "category": "tool",
            "keywords": ["hippo", "recent"],
            "actions": [
                {
                    "id": "default",
                    "label": "Recall",
                    "kind": "tool_call",
                    "requires_confirmation": False,
                }
            ],
        })

    return {"items": items}


def handle(request):
    method = request.get("method")
    request_id = request.get("id")

    if method == "initialize":
        send_response(
            request_id,
            {
                "plugin_id": "hippo.bridge",
                "plugin_label": "Hippo Memory Bridge",
                "capabilities": ["catalog", "suggest", "execute"],
            },
        )
    elif method == "catalog":
        send_response(request_id, static_catalog_items())
    elif method == "suggest":
        params = request.get("params") or {}
        query = params.get("query", "")
        trimmed = query.strip().lower()

        recall_prefix = None
        for prefix in ("recall ", "mem ", "hippo "):
            if trimmed.startswith(prefix):
                recall_prefix = prefix
                break

        if recall_prefix:
            search_query = query[len(recall_prefix):].strip()
            if search_query:
                send_response(request_id, search_memory(search_query))
            else:
                send_response(request_id, static_catalog_items())
        elif "hippo".startswith(trimmed) or trimmed in ("recall", "mem"):
            send_response(request_id, static_catalog_items())
        else:
            send_response(request_id, {"items": []})
    elif method == "execute":
        params = request.get("params") or {}
        item_id = params.get("item_id", "")
        query = params.get("query", "")

        if item_id == "hippo.timeline":
            send_response(request_id, timeline_items())
        elif item_id == "hippo.recall" or item_id.startswith("hippo.node:"):
            search_query = query
            for prefix in ("recall ", "mem ", "hippo "):
                if search_query.lower().startswith(prefix):
                    search_query = search_query[len(prefix):].strip()
                    break
            items = search_memory(search_query).get("items", [])
            send_response(request_id, {"message": f"Found {len(items)} memory item(s)"})
        else:
            send_response(request_id, {"message": f"Executed {item_id}"})
    elif method == "shutdown":
        send_response(request_id, {"message": "shutdown"})
        return False
    else:
        send_response(
            request_id,
            error={"code": -32601, "message": f"unknown method: {method}"},
        )

    return True


def main():
    for raw in sys.stdin:
        raw = raw.strip()
        if not raw:
            continue
        try:
            keep_running = handle(json.loads(raw))
        except Exception as exc:
            send_response(None, error={"code": -32000, "message": str(exc)})
            keep_running = True
        if not keep_running:
            break


if __name__ == "__main__":
    main()
