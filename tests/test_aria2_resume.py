"""Local HTTP Range server test: pause, resume, and verify full file integrity.

The test uses no external network connection. A deliberately slow loopback server
ensures a transfer remains incomplete long enough to pause it.
"""
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


PAYLOAD = bytes(range(256)) * 16384  # 4 MiB, deterministic content


class SlowRangeHandler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_GET(self):
        if self.path != "/range.bin":
            self.send_error(404)
            return
        start, end = 0, len(PAYLOAD) - 1
        header = self.headers.get("Range")
        if header:
            if not header.startswith("bytes="):
                self.send_error(416)
                return
            bounds = header[6:].split("-", 1)
            try:
                start = int(bounds[0])
                end = int(bounds[1]) if bounds[1] else end
            except ValueError:
                self.send_error(416)
                return
            if start >= len(PAYLOAD) or end < start:
                self.send_error(416)
                return
            end = min(end, len(PAYLOAD) - 1)
        length = end - start + 1
        self.send_response(206 if header else 200)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Content-Length", str(length))
        if header:
            self.send_header("Content-Range", f"bytes {start}-{end}/{len(PAYLOAD)}")
        self.end_headers()
        try:
            for cursor in range(start, end + 1, 8192):
                self.wfile.write(PAYLOAD[cursor:min(cursor + 8192, end + 1)])
                self.wfile.flush()
                time.sleep(0.009)
        except (BrokenPipeError, ConnectionResetError):
            pass

    def log_message(self, *_args):
        pass


class TestHttpResume(unittest.TestCase):
    def test_pause_resume_preserves_checksum(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = pathlib.Path(directory)
            server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), SlowRangeHandler)
            server.daemon_threads = True
            threading.Thread(target=server.serve_forever, daemon=True).start()

            with socket.socket() as probe:
                probe.bind(("127.0.0.1", 0))
                port = probe.getsockname()[1]
            token = "local-pause-resume"
            process = subprocess.Popen(
                [
                    "aria2c", "--no-conf=true", "--enable-rpc=true",
                    "--rpc-listen-all=false", f"--rpc-listen-port={port}",
                    f"--rpc-secret={token}", "--console-log-level=warn",
                    "--split=1", "--max-connection-per-server=1",
                    f"--dir={folder}",
                ],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                stdin=subprocess.DEVNULL,
            )

            def rpc(method, args=None):
                payload = json.dumps({
                    "jsonrpc": "2.0", "id": "resume-check",
                    "method": "aria2." + method,
                    "params": ["token:" + token] + (args or []),
                }).encode()
                request = urllib.request.Request(
                    f"http://127.0.0.1:{port}/jsonrpc",
                    data=payload, headers={"Content-Type": "application/json"},
                )
                with urllib.request.urlopen(request, timeout=3) as response:
                    data = json.load(response)
                if "error" in data:
                    raise AssertionError(data["error"])
                return data["result"]

            try:
                deadline = time.monotonic() + 15
                while True:
                    try:
                        rpc("getVersion")
                        break
                    except (OSError, urllib.error.URLError):
                        if process.poll() is not None:
                            self.fail("aria2c exited during startup")
                        if time.monotonic() >= deadline:
                            self.fail("aria2 RPC startup timed out")
                        time.sleep(0.1)

                gid = rpc("addUri", [[f"http://127.0.0.1:{server.server_port}/range.bin"], {
                    "split": "1", "max-connection-per-server": "1",
                }])
                deadline = time.monotonic() + 15
                while time.monotonic() < deadline:
                    state = rpc("tellStatus", [gid])
                    completed = int(state.get("completedLength", "0"))
                    if 64 * 1024 < completed < len(PAYLOAD):
                        break
                    if state["status"] in ("error", "removed", "complete"):
                        self.fail("Transfer finished or failed before pause could be tested")
                    time.sleep(0.05)
                else:
                    self.fail("Slow transfer did not reach a partial state")

                rpc("pause", [gid])
                deadline = time.monotonic() + 10
                while time.monotonic() < deadline:
                    state = rpc("tellStatus", [gid])
                    if state["status"] == "paused":
                        break
                    time.sleep(0.05)
                else:
                    self.fail("aria2 did not enter paused state")
                partial_size = int(state["completedLength"])
                self.assertGreater(partial_size, 0)
                self.assertLess(partial_size, len(PAYLOAD))

                rpc("unpause", [gid])
                deadline = time.monotonic() + 35
                while time.monotonic() < deadline:
                    state = rpc("tellStatus", [gid])
                    if state["status"] == "complete":
                        break
                    if state["status"] in ("error", "removed"):
                        self.fail(f"Transfer failed after resume: {state}")
                    time.sleep(0.1)
                else:
                    self.fail("Resumed transfer did not finish")
                actual = (folder / "range.bin").read_bytes()
                self.assertEqual(hashlib.sha256(actual).digest(), hashlib.sha256(PAYLOAD).digest())
                self.assertEqual(int(state["completedLength"]), len(PAYLOAD))
            finally:
                server.shutdown()
                server.server_close()
                if process.poll() is None:
                    process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)


if __name__ == "__main__":
    unittest.main()
