#!/usr/bin/env python3
"""Run a clean-environment LSP lifecycle smoke test against a release archive."""

from __future__ import annotations

import argparse
import json
import os
import queue
import shutil
import subprocess
import sys
import tarfile
import tempfile
import threading
import time
import zipfile
from pathlib import Path, PurePosixPath


def send(stream: object, message: dict[str, object]) -> None:
    body = json.dumps(message, separators=(",", ":")).encode("utf-8")
    stream.write(f"Content-Length: {len(body)}\r\n\r\n".encode("ascii") + body)
    stream.flush()


def read_messages(stream: object, messages: queue.Queue[dict[str, object]]) -> None:
    while True:
        headers: dict[str, str] = {}
        while True:
            line = stream.readline()
            if not line:
                return
            if line in (b"\n", b"\r\n"):
                break
            name, _, value = line.decode("ascii").partition(":")
            headers[name.lower()] = value.strip()
        length = int(headers["content-length"])
        messages.put(json.loads(stream.read(length)))


def receive(messages: queue.Queue[dict[str, object]], predicate: object) -> dict[str, object]:
    deadline = time.monotonic() + 10
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise SystemExit("timed out waiting for LSP message")
        message = messages.get(timeout=remaining)
        if predicate(message):
            return message


def extract(archive: Path, destination: Path) -> Path:
    if archive.suffix == ".zip":
        with zipfile.ZipFile(archive) as contents:
            names = [member.filename for member in contents.infolist()]
            if any(PurePosixPath(name).is_absolute() or ".." in PurePosixPath(name).parts for name in names):
                raise SystemExit("release archive contains an unsafe path")
            contents.extractall(destination)
    else:
        with tarfile.open(archive, "r:gz") as contents:
            members = contents.getmembers()
            if any(
                PurePosixPath(member.name).is_absolute()
                or ".." in PurePosixPath(member.name).parts
                or member.issym()
                or member.islnk()
                for member in members
            ):
                raise SystemExit("release archive contains an unsafe path")
            contents.extractall(destination)
    roots = [path for path in destination.iterdir() if path.is_dir()]
    if len(roots) != 1:
        raise SystemExit(f"release archive should have one root directory, got {roots}")
    return roots[0]


def clean_environment(home: Path) -> dict[str, str]:
    environment = {"HOME": str(home), "TMPDIR": str(home), "TEMP": str(home), "TMP": str(home), "LANG": "C"}
    if sys.platform == "win32":
        for name in ("SystemRoot", "COMSPEC"):
            if value := os.environ.get(name):
                environment[name] = value
    return environment


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifact", type=Path, required=True)
    parser.add_argument("--binary-name", required=True)
    args = parser.parse_args()

    with tempfile.TemporaryDirectory(prefix="ferrolex-lsp-smoke-") as temporary:
        temporary_path = Path(temporary)
        server = extract(args.artifact, temporary_path) / args.binary_name
        if sys.platform != "win32":
            server.chmod(server.stat().st_mode | 0o100)
        process = subprocess.Popen(
            [str(server)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            cwd=temporary_path,
            env=clean_environment(temporary_path),
        )
        assert process.stdin is not None
        assert process.stdout is not None
        messages: queue.Queue[dict[str, object]] = queue.Queue()
        threading.Thread(target=read_messages, args=(process.stdout, messages), daemon=True).start()

        send(process.stdin, {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"initializationOptions": {"ferrolex": {"words": ["known"]}}}})
        initialized = receive(messages, lambda message: message.get("id") == 1)
        if initialized.get("result", {}).get("serverInfo", {}).get("name") != "ferrolex-lsp":
            raise SystemExit(f"invalid initialize response: {initialized}")
        send(process.stdin, {"jsonrpc": "2.0", "method": "initialized", "params": {}})
        uri = "file:///smoke.txt"
        send(process.stdin, {"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {"textDocument": {"uri": uri, "languageId": "text", "version": 1, "text": "known typo"}}})
        diagnostics = receive(messages, lambda message: message.get("method") == "textDocument/publishDiagnostics")
        published = diagnostics.get("params", {})
        if published.get("uri") != uri or published.get("diagnostics", [{}])[0].get("data", {}).get("word") != "typo":
            raise SystemExit(f"invalid diagnostics response: {diagnostics}")
        send(process.stdin, {"jsonrpc": "2.0", "id": 2, "method": "shutdown", "params": None})
        shutdown = receive(messages, lambda message: message.get("id") == 2)
        if shutdown.get("result") is not None:
            raise SystemExit(f"invalid shutdown response: {shutdown}")
        send(process.stdin, {"jsonrpc": "2.0", "method": "exit", "params": {}})
        if process.wait(timeout=10) != 0:
            raise SystemExit(process.stderr.read().decode("utf-8", errors="replace"))
    print("ferrolex-lsp stdio smoke test passed")


if __name__ == "__main__":
    main()
