"""A minimal LSP client for driving sqformat-lsp over stdio.

These checks talk raw JSON-RPC on purpose: they exercise the protocol boundary the editor uses,
which the Rust unit tests cannot reach.
"""

import json
import subprocess
from pathlib import Path


def file_uri(path):
    return Path(path).resolve().as_uri()


class LspClient:
    def __init__(
        self, server_path, root=None, capabilities=None, initialization_options=None
    ):
        self.process = subprocess.Popen(
            [str(server_path)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
        )
        self.next_id = 1
        params = {"processId": None, "capabilities": capabilities or {}}
        if initialization_options is not None:
            params["initializationOptions"] = initialization_options
        if root is not None:
            params["rootUri"] = file_uri(root)
        self.initialize_result = self.request("initialize", params)
        self.notify("initialized", {})
        self._wait_until_initialized()

    def _wait_until_initialized(self):
        """Drains startup traffic so later checks cannot consume stale diagnostics."""
        while True:
            message = self.receive()
            if "id" in message and "method" in message:
                self.send({"jsonrpc": "2.0", "id": message["id"], "result": None})
            if (
                message.get("method") == "window/logMessage"
                and message["params"]["message"].startswith(
                    "sqformat language server initialized"
                )
            ):
                return

    def send(self, message):
        body = json.dumps(message).encode()
        self.process.stdin.write(f"Content-Length: {len(body)}\r\n\r\n".encode() + body)
        self.process.stdin.flush()

    def receive(self):
        length = None
        while True:
            header = self.process.stdout.readline().decode().strip()
            if header.lower().startswith("content-length:"):
                length = int(header.split(":")[1])
            elif header == "":
                break
        return json.loads(self.process.stdout.read(length))

    def notify(self, method, params):
        self.send({"jsonrpc": "2.0", "method": method, "params": params})

    def request(self, method, params, on_server_request=None):
        request_id = self.next_id
        self.next_id += 1
        self.send({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params})
        while True:
            message = self.receive()
            if message.get("id") == request_id and "method" not in message:
                return message.get("result")
            # The server may ask the client something mid-request; a real client answers.
            if "id" in message and "method" in message:
                if on_server_request:
                    on_server_request(message)
                self.send({"jsonrpc": "2.0", "id": message["id"], "result": None})

    def open(self, path, text=None, version=1):
        text = open(path).read() if text is None else text
        self.notify(
            "textDocument/didOpen",
            {
                "textDocument": {
                    "uri": file_uri(path),
                    "languageId": "squirrel",
                    "version": version,
                    "text": text,
                }
            },
        )
        return text

    def close(self, path):
        self.notify("textDocument/didClose", {"textDocument": {"uri": file_uri(path)}})

    def change(self, path, text, version):
        self.notify(
            "textDocument/didChange",
            {
                "textDocument": {"uri": file_uri(path), "version": version},
                "contentChanges": [{"text": text}],
            },
        )

    def completion(self, path, text, offset):
        result = self.request(
            "textDocument/completion",
            {"textDocument": {"uri": file_uri(path)}, "position": position(text, offset)},
        )
        if result is None:
            return []
        return result["items"] if isinstance(result, dict) else result

    def signature_help(self, path, text, offset):
        result = self.request(
            "textDocument/signatureHelp",
            {"textDocument": {"uri": file_uri(path)}, "position": position(text, offset)},
        )
        return [signature["label"] for signature in result["signatures"]] if result else []

    def references(self, path, text, offset):
        return self.request(
            "textDocument/references",
            {
                "textDocument": {"uri": file_uri(path)},
                "position": position(text, offset),
                "context": {"includeDeclaration": True},
            },
        ) or []

    def prepare_rename(self, path, text, offset):
        return self.request(
            "textDocument/prepareRename",
            {
                "textDocument": {"uri": file_uri(path)},
                "position": position(text, offset),
            },
        )

    def rename(self, path, text, offset, new_name):
        return self.request(
            "textDocument/rename",
            {
                "textDocument": {"uri": file_uri(path)},
                "position": position(text, offset),
                "newName": new_name,
            },
        )

    def diagnostics(self, path, version=None):
        """Waits for the diagnostics published for `path`, answering anything asked meanwhile."""
        uri = file_uri(path)
        while True:
            message = self.receive()
            if message.get("method") == "textDocument/publishDiagnostics":
                if message["params"]["uri"] == uri and (
                    version is None or message["params"].get("version") == version
                ):
                    return message["params"]["diagnostics"]
            elif "id" in message and "method" in message:
                self.send({"jsonrpc": "2.0", "id": message["id"], "result": None})

    def semantic_tokens(self, path):
        result = self.request(
            "textDocument/semanticTokens/full",
            {"textDocument": {"uri": file_uri(path)}},
        )
        return result["data"] if result else []

    def formatting(self, path, options=None):
        result = self.request(
            "textDocument/formatting",
            {
                "textDocument": {"uri": file_uri(path)},
                "options": options
                or {"tabSize": 4, "insertSpaces": False},
            },
        )
        return result or []

    def shutdown(self, timeout=10):
        """Shuts down and returns the process exit code, or None if it did not exit."""
        self.request("shutdown", None)
        self.notify("exit", None)
        try:
            return self.process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            self.process.kill()
            return None


def position(text, offset):
    line = text[:offset].count("\n")
    start = text[:offset].rindex("\n") + 1 if line else 0
    character = len(text[start:offset].encode("utf-16-le")) // 2
    return {"line": line, "character": character}


def labels(items):
    return sorted(item["label"] for item in items)


def decode_tokens(data):
    """Expands the delta encoding into (line, start, length, type, modifiers) tuples."""
    tokens = []
    line = character = 0
    for index in range(0, len(data), 5):
        delta_line, delta_start, length, token_type, modifiers = data[index : index + 5]
        line += delta_line
        character = delta_start if delta_line else character + delta_start
        tokens.append((line, character, length, token_type, modifiers))
    return tokens
