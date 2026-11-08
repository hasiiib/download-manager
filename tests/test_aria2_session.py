"""Check that aria2 saves paused downloads and restores their original GIDs.

Runs entirely on localhost and does not download any internet content.
"""
import json
import pathlib
import socket
import subprocess
import tempfile
import time
import unittest
import urllib.error
import urllib.request


def available_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class Aria2Session:
    def __init__(self, folder, session_file):
        self.port = available_port()
        self.secret = "local-recovery-test"
        self.process = subprocess.Popen(
            [
                "aria2c",
                "--no-conf=true",
                "--enable-rpc=true",
                "--rpc-listen-all=false",
                f"--rpc-listen-port={self.port}",
                f"--rpc-secret={self.secret}",
                "--console-log-level=warn",
                f"--dir={folder}",
                f"--input-file={session_file}",
                f"--save-session={session_file}",
                "--save-session-interval=1",
                "--max-concurrent-downloads=1",
            ],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

    def rpc(self, method, args=None):
        payload = json.dumps({
            "jsonrpc": "2.0",
            "id": "session-test",
            "method": "aria2." + method,
            "params": ["token:" + self.secret] + (args or []),
        }).encode("utf-8")
        request = urllib.request.Request(
            f"http://127.0.0.1:{self.port}/jsonrpc",
            data=payload,
            headers={"Content-Type": "application/json"},
        )
        with urllib.request.urlopen(request, timeout=2) as response:
            result = json.load(response)
        if result.get("error"):
            raise AssertionError(f"aria2.{method} failed: {result['error']}")
        return result["result"]

    def ready(self):
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                raise AssertionError("aria2 exited before its RPC endpoint was ready")
            try:
                self.rpc("getVersion")
                return
            except (OSError, urllib.error.URLError):
                time.sleep(0.1)
        raise AssertionError("aria2 RPC startup timed out")

    def stop(self):
        if self.process.poll() is not None:
            return
        try:
            self.rpc("shutdown")
            self.process.wait(timeout=5)
        except (OSError, AssertionError, subprocess.TimeoutExpired):
            self.process.terminate()
            try:
                self.process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=3)


class TestSessionRecovery(unittest.TestCase):
    def test_paused_job_is_restored_with_same_gid(self):
        with tempfile.TemporaryDirectory() as tmp:
            folder = pathlib.Path(tmp)
            session_file = folder / "aria2.session"
            session_file.write_text("", encoding="utf-8")
            source = "http://127.0.0.1:9/recovery-test-file.bin"

            first = Aria2Session(folder, session_file)
            try:
                first.ready()
                gid = first.rpc("addUri", [[source], {"pause": "true"}])
                self.assertEqual(first.rpc("tellStatus", [gid])["status"], "paused")
                self.assertEqual(first.rpc("saveSession"), "OK")
                self.assertIn(gid, session_file.read_text(encoding="utf-8"))
            finally:
                first.stop()

            second = Aria2Session(folder, session_file)
            try:
                second.ready()
                state = second.rpc("tellStatus", [gid])
                self.assertEqual(state["gid"], gid)
                self.assertIn(state["status"], ("paused", "waiting"))
                self.assertEqual(second.rpc("getUris", [gid])[0]["uri"], source)
            finally:
                second.stop()


if __name__ == "__main__":
    unittest.main()
