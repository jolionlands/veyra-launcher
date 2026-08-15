#!/usr/bin/env python3
import json
import sys


def send_response(request_id, result=None, error=None):
    response = {"jsonrpc": "2.0", "id": request_id}
    if error is not None:
        response["error"] = error
    else:
        response["result"] = result
    print(json.dumps(response), flush=True)


def catalog_items():
    return {
        "items": [
            {
                "id": "echo",
                "label": "Sample Plugin: Echo Query",
                "subtitle": "Return the current Veyra query through JSON-RPC stdio",
                "category": "tool",
                "keywords": ["sample", "plugin", "jsonrpc", "echo"],
                "actions": [
                    {
                        "id": "default",
                        "label": "Echo",
                        "kind": "tool_call",
                        "requires_confirmation": False,
                    }
                ],
            }
        ]
    }


def handle(request):
    method = request.get("method")
    request_id = request.get("id")

    if method == "initialize":
        send_response(
            request_id,
            {
                "plugin_id": "sample.echo",
                "plugin_label": "Sample Echo Plugin",
                "capabilities": ["catalog", "suggest", "execute"],
            },
        )
    elif method == "catalog":
        send_response(request_id, catalog_items())
    elif method == "suggest":
        params = request.get("params") or {}
        query = params.get("query") or ""
        if query.strip():
            item = catalog_items()["items"][0]
            item["label"] = f"Sample Plugin: Echo '{query}'"
            send_response(request_id, {"items": [item]})
        else:
            send_response(request_id, {"items": []})
    elif method == "execute":
        params = request.get("params") or {}
        query = params.get("query") or ""
        send_response(request_id, {"message": f"Sample plugin received: {query}"})
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
