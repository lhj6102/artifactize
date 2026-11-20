"""Subprocess seams for the plugin onramp's disposable host ownership."""
from pathlib import Path
import os
import runpy
import socket
import subprocess
import sys
import tempfile
import unittest

plugin = runpy.run_path(str(Path(__file__).with_name("agent_plugin.py")))
# A fixture watchdog prevents a dead child or a failed drain hanging the suite.
WATCHDOG = 10


@unittest.skipUnless(os.name == "posix", "The disposable host check supports Unix hosts only.")
class HostOwnership(unittest.TestCase):
    def test_host_status_and_output_reach_the_caller(self):
        with tempfile.TemporaryDirectory() as directory:
            output = plugin["run"]([sys.executable, "-c", "print('host output')"], os.environ, directory)
        self.assertEqual(output, "host output\n")

    @unittest.skipUnless(sys.platform == "linux", "The Linux owner also adopts a detached session.")
    def test_detached_descendant_does_not_keep_the_owned_pipe_open(self):
        receive, inherited = socket.socketpair()
        with receive, inherited, tempfile.TemporaryDirectory() as directory:
            receive.settimeout(WATCHDOG)
            command = """
import os, subprocess, sys
fd = int(sys.argv[1])
subprocess.Popen([sys.executable, '-c', 'import os,signal,sys; os.write(int(sys.argv[1]),b"ready"); signal.pause()', sys.argv[1]],
                 pass_fds=(fd,), start_new_session=True)
"""
            with plugin["host_command"]([sys.executable, "-c", command, str(inherited.fileno())],
                                        os.environ, directory, pass_fds=(inherited.fileno(),)) as process:
                inherited.close()
                with receive.makefile("rb") as signal:
                    self.assertEqual(signal.read(5), b"ready")
                self.assertEqual(process.wait(timeout=WATCHDOG), 0)
            self.assertEqual(receive.recv(1), b"")


if __name__ == "__main__":
    unittest.main()
