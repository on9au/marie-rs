#!/usr/bin/env python3
"""Drive marie-lsp over stdio and print what it answers.

For checking the server independently of any editor. Give it a .mas file, or let it
use examples/demo.mas.
"""

import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent


def find_server() -> str:
    for build in ("release", "debug"):
        candidate = ROOT / "target" / build / "marie-lsp"
        if candidate.exists():
            return str(candidate)
    return "marie-lsp"


class Server:
    def __init__(self, command: str) -> None:
        self.proc = subprocess.Popen(
            [command],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
        )

    def send(self, message: dict) -> None:
        body = json.dumps(message).encode()
        self.proc.stdin.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
        self.proc.stdin.flush()

    def receive(self) -> dict:
        length = None
        while True:
            line = self.proc.stdout.readline()
            if not line or line in (b"\r\n", b"\n"):
                break
            if line.lower().startswith(b"content-length:"):
                length = int(line.split(b":")[1])
        if length is None:
            raise SystemExit("the server closed the connection")
        return json.loads(self.proc.stdout.read(length))

    def request(self, id: int, method: str, params: dict) -> dict:
        self.send({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
        return self.receive()


def main() -> None:
    path = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "examples/demo.mas"
    source = path.read_text()
    uri = path.resolve().as_uri()

    server = Server(find_server())
    reply = server.request(1, "initialize", {
        "processId": None,
        "rootUri": None,
        "capabilities": {"general": {"positionEncodings": ["utf-8", "utf-16"]}},
    })
    info = reply["result"]["serverInfo"]
    print(f"{info['name']} {info['version']}, "
          f"encoding {reply['result']['capabilities']['positionEncoding']}\n")
    server.send({"jsonrpc": "2.0", "method": "initialized", "params": {}})

    server.send({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
        "textDocument": {"uri": uri, "languageId": "marie", "version": 1,
                         "text": source}}})
    published = server.receive()["params"]["diagnostics"]

    print(f"diagnostics ({len(published)}):")
    for d in published:
        line = d["range"]["start"]["line"] + 1
        print(f"  line {line:>3}  [{d['code']}] {d['message'].splitlines()[0]}")

    if published:
        first = published[0]["range"]
        actions = server.request(2, "textDocument/codeAction", {
            "textDocument": {"uri": uri}, "range": first,
            "context": {"diagnostics": []}})["result"]
        print(f"\nquick fixes for the first finding ({len(actions)}):")
        for action in actions:
            print(f"  {action['title']}")

    hints = server.request(3, "textDocument/inlayHint", {
        "textDocument": {"uri": uri},
        "range": {"start": {"line": 0, "character": 0},
                  "end": {"line": len(source.splitlines()) + 1, "character": 0}},
    })["result"]
    print(f"\ninlay hints ({len(hints)}):"
          + ("  (none: the file does not assemble yet)" if not hints else ""))
    for hint in hints[:8]:
        print(f"  {hint['label']}")

    server.request(99, "shutdown", None)
    server.send({"jsonrpc": "2.0", "method": "exit", "params": None})
    server.proc.stdin.close()
    server.proc.wait(timeout=5)


if __name__ == "__main__":
    main()
