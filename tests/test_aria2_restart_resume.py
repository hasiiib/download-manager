"""Verify partial HTTP download survives a complete aria2 process restart.

Uses the local slow byte-range server and the session helper from the other
integration tests. No external network access or copyrighted material.
"""
import hashlib
import http.server
import pathlib
import tempfile
import threading
import time
import unittest

from test_aria2_resume import PAYLOAD, SlowRangeHandler
from test_aria2_session import Aria2Session


def wait_for_status(engine, gid, expected, timeout=15):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        current = engine.rpc("tellStatus", [gid])
        if current.get("status") in expected:
            return current
        if current.get("status") in ("error", "removed"):
            raise AssertionError(f"Download entered unexpected state: {current}")
        time.sleep(0.05)
    raise AssertionError(f"Timeout waiting for one of {expected}")


class TestHttpRestartResume(unittest.TestCase):
    def test_restart_resumes_partial_http_download(self):
        with tempfile.TemporaryDirectory() as tmp:
            folder = pathlib.Path(tmp)
            session = folder / "aria2.session"
            session.write_text("", encoding="utf-8")
            server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), SlowRangeHandler)
            server.daemon_threads = True
            threading.Thread(target=server.serve_forever, daemon=True).start()
            source = f"http://127.0.0.1:{server.server_port}/range.bin"
            gid = None

            try:
                first = Aria2Session(folder, session)
                try:
                    first.ready()
                    gid = first.rpc("addUri", [[source], {
                        "pause": "true", "split": "1",
                        "max-connection-per-server": "1",
                    }])
                    first.rpc("unpause", [gid])
                    end = time.monotonic() + 15
                    while time.monotonic() < end:
                        state = first.rpc("tellStatus", [gid])
                        completed = int(state.get("completedLength", "0"))
                        if 65536 < completed < len(PAYLOAD):
                            break
                        if state["status"] in ("error", "removed", "complete"):
                            self.fail("Transfer completed or failed before interrupted state")
                        time.sleep(0.05)
                    else:
                        self.fail("Partial transfer did not progress")
                    first.rpc("pause", [gid])
                    paused = wait_for_status(first, gid, {"paused"}, timeout=10)
                    self.assertGreater(int(paused["completedLength"]), 0)
                    self.assertLess(int(paused["completedLength"]), len(PAYLOAD))
                    self.assertEqual(first.rpc("saveSession"), "OK")
                    self.assertIn(gid, session.read_text(encoding="utf-8"))
                finally:
                    first.stop()

                second = Aria2Session(folder, session)
                try:
                    second.ready()
                    state = second.rpc("tellStatus", [gid])
                    self.assertEqual(state["gid"], gid)
                    self.assertIn(state["status"], ("paused", "waiting"))
                    second.rpc("unpause", [gid])
                    state = wait_for_status(second, gid, {"complete"}, timeout=40)
                    self.assertEqual(int(state["completedLength"]), len(PAYLOAD))
                    target = folder / "range.bin"
                    self.assertEqual(hashlib.sha256(target.read_bytes()).digest(),
                                     hashlib.sha256(PAYLOAD).digest())
                finally:
                    second.stop()
            finally:
                server.shutdown()
                server.server_close()


if __name__ == "__main__":
    unittest.main()
