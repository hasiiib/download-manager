"""Local-only aria2 RPC smoke test. No third-party network access required."""
import functools
import hashlib
import http.server
import json
import pathlib
import socket
import subprocess
import tempfile
import threading
import time
import unittest
import urllib.error
import urllib.request


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass


class TestAria2Rpc(unittest.TestCase):
    def test_local_file_transfer_via_authenticated_rpc(self):
        with tempfile.TemporaryDirectory() as tmp:
            base = pathlib.Path(tmp)
            source = base / "source"
            dest = base / "downloads"
            source.mkdir()
            dest.mkdir()
            payload = bytes(range(256)) * 4096
            (source / "payload.bin").write_bytes(payload)

            handler = functools.partial(QuietHandler, directory=str(source))
            server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            port = free_port()
            secret = "ci-test-secret"
            process = subprocess.Popen(
                [
                    "aria2c",
                    "--no-conf=true",
                    "--enable-rpc=true",
                    "--rpc-listen-all=false",
                    f"--rpc-listen-port={port}",
                    f"--rpc-secret={secret}",
                    "--console-log-level=warn",
                    "--max-concurrent-downloads=1",
                    f"--dir={dest}",
                ],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.PIPE,
            )

            def rpc(method, arguments=None):
                params = [f"token:{secret}"] + (arguments or [])
                body = json.dumps({
                    "jsonrpc": "2.0",
                    "id": "test",
                    "method": "aria2." + method,
                    "params": params,
                }).encode()
                request = urllib.request.Request(
                    f"http://127.0.0.1:{port}/jsonrpc",
                    data=body,
                    headers={"Content-Type": "application/json"},
                )
                with urllib.request.urlopen(request, timeout=2) as response:
                    data = json.load(response)
                if "error" in data:
                    raise AssertionError(f"aria2 RPC error: {data['error']}")
                return data["result"]

            try:
                deadline = time.monotonic() + 15
                while True:
                    try:
                        rpc("getVersion")
                        break
                    except (urllib.error.URLError, TimeoutError):
                        if process.poll() is not None:
                            self.fail("aria2c exited before RPC startup")
                        if time.monotonic() > deadline:
                            self.fail("aria2 RPC never became ready")
                        time.sleep(0.1)
                gid = rpc("addUri", [[f"http://127.0.0.1:{server.server_port}/payload.bin"], {
                    "dir": str(dest),
                    "split": "2",
                    "max-connection-per-server": "2",
                }])
                deadline = time.monotonic() + 25
                while time.monotonic() < deadline:
                    status = rpc("tellStatus", [gid])
                    if status["status"] == "complete":
                        break
                    if status["status"] in ("error", "removed"):
                        self.fail(f"Unexpected aria2 status: {status}")
                    time.sleep(0.1)
                else:
                    self.fail("Local transfer did not finish within timeout")
                downloaded = (dest / "payload.bin").read_bytes()
                self.assertEqual(hashlib.sha256(downloaded).digest(), hashlib.sha256(payload).digest())
                self.assertEqual(int(status["completedLength"]), len(payload))
            finally:
                server.shutdown()
                server.server_close()
                if process.poll() is None:
                    process.terminate()
                try:
                    process.communicate(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.communicate(timeout=5)


if __name__ == "__main__":
    unittest.main()
