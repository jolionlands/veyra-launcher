#!/usr/bin/env python3
"""Veyra JSON-RPC plugin bridge for Nonorganize.

Requires a running Nonorganize HTTP server (e.g. `nonorganize serve --port 8080`).
Add to your Veyra `plugins.toml`:

[[plugins]]
id = "nonorganize"
label = "Nonorganize"
kind = "json_rpc_stdio"
command = "python"
args = ["C:\\Path\\To\\nonorganize-veyra-bridge.py"]
keywords = ["nonorganize", "files", "search", "organize"]
enabled = true
"""

import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request

DEFAULT_HOST = os.environ.get("NONORGANIZE_HOST", "127.0.0.1:8080")


def nonorganize_url(path: str) -> str:
    return f"http://{DEFAULT_HOST}{path}"


def http_get(path: str) -> tuple[int, str]:
    try:
        with urllib.request.urlopen(nonorganize_url(path), timeout=2.0) as resp:
            return resp.status, resp.read().decode("utf-8", errors="replace")
    except urllib.error.HTTPError as exc:
        return exc.code, exc.read().decode("utf-8", errors="replace")
    except Exception as exc:
        return 0, str(exc)


def parse_json_body(raw: str) -> dict | list:
    # Nonorganize returns raw HTTP with a JSON body.
    if "\r\n\r\n" in raw:
        _, body = raw.split("\r\n\r\n", 1)
    elif "\n\n" in raw:
        _, body = raw.split("\n\n", 1)
    else:
        body = raw
    return json.loads(body)


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
                "id": "nonorganize.search",
                "label": "Nonorganize: Search files",
                "subtitle": "Search the Nonorganize index",
                "category": "tool",
                "keywords": ["nonorganize", "files", "search"],
                "actions": [
                    {
                        "id": "default",
                        "label": "Search",
                        "kind": "tool_call",
                        "requires_confirmation": False,
                    }
                ],
            },
            {
                "id": "nonorganize.stats",
                "label": "Nonorganize: Show stats",
                "subtitle": "Display index statistics",
                "category": "tool",
                "keywords": ["nonorganize", "stats"],
                "actions": [
                    {
                        "id": "default",
                        "label": "Stats",
                        "kind": "tool_call",
                        "requires_confirmation": False,
                    }
                ],
            },
        ]
    }


def search_items(query: str):
    """Query Nonorganize /search and return Veyra catalog items."""
    status, raw = http_get(f"/search?q={urllib.parse.quote(query)}")
    if status != 200:
        return {
            "items": [
                {
                    "id": "nonorganize.error",
                    "label": "Nonorganize search failed",
                    "subtitle": raw[:120],
                    "category": "tool",
                    "keywords": [],
                    "actions": [],
                }
            ]
        }

    try:
        results = parse_json_body(raw)
    except Exception as exc:
        return {
            "items": [
                {
                    "id": "nonorganize.error",
                    "label": "Nonorganize response error",
                    "subtitle": str(exc),
                    "category": "tool",
                    "keywords": [],
                    "actions": [],
                }
            ]
        }

    items = []
    for result in results[:10]:
        path = result.get("path", "")
        score = result.get("score", 0.0)
        if not path:
            continue
        items.append({
            "id": f"nonorganize.file:{path}",
            "label": os.path.basename(path) or path,
            "subtitle": f"{path}  (score: {score:.3f})",
            "category": "file",
            "keywords": ["nonorganize", os.path.basename(path)],
            "actions": [
                {
                    "id": "open",
                    "label": "Open file",
                    "kind": "open_file",
                    "command": path,
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
                "plugin_id": "nonorganize.bridge",
                "plugin_label": "Nonorganize Bridge",
                "capabilities": ["catalog", "suggest", "execute"],
            },
        )
    elif method == "catalog":
        send_response(request_id, static_catalog_items())
    elif method == "suggest":
        params = request.get("params") or {}
        query = params.get("query", "")
        trimmed = query.strip().lower()

        # Activate on "no <query>" or "non <query>" prefixes.
        search_prefix = None
        for prefix in ("no ", "non "):
            if trimmed.startswith(prefix):
                search_prefix = prefix
                break

        if search_prefix:
            search_query = query[len(search_prefix):].strip()
            if search_query:
                send_response(request_id, search_items(search_query))
            else:
                send_response(request_id, static_catalog_items())
        elif "nonorganize".startswith(trimmed) or trimmed == "no":
            send_response(request_id, static_catalog_items())
        else:
            send_response(request_id, {"items": []})
    elif method == "execute":
        params = request.get("params") or {}
        item_id = params.get("item_id", "")
        query = params.get("query", "")

        if item_id == "nonorganize.stats":
            status, raw = http_get("/stats")
            try:
                stats = parse_json_body(raw) if status == 200 else {"error": raw}
                message = json.dumps(stats, indent=2)
            except Exception as exc:
                message = f"Stats error: {exc}"
            send_response(request_id, {"message": message})
        elif item_id == "nonorganize.search":
            # Re-run search with the current query (minus prefix).
            search_query = query
            for prefix in ("no ", "non "):
                if search_query.lower().startswith(prefix):
                    search_query = search_query[len(prefix):].strip()
                    break
            items = search_items(search_query).get("items", [])
            send_response(request_id, {"message": f"Found {len(items)} result(s)"})
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
