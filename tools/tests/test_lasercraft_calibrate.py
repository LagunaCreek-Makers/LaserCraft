"""Dependency-free CLI validation tests for the offline camera calibration helper."""
import subprocess
import sys
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / 'lasercraft_calibrate.py'

class CalibrationCliTests(unittest.TestCase):
    def run_cli(self, *args):
        return subprocess.run([sys.executable, str(SCRIPT), *args], capture_output=True, text=True)

    def test_help(self):
        result = self.run_cli('--help')
        self.assertEqual(result.returncode, 0)
        self.assertIn('--square-mm', result.stdout)

    def test_rejects_invalid_grid(self):
        result = self.run_cli('missing.jpg', '--columns', '2', '--rows', '6', '--square-mm', '20')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Invalid target parameters', result.stderr)

    def test_rejects_invalid_square_size(self):
        result = self.run_cli('missing.jpg', '--columns', '9', '--rows', '6', '--square-mm', '0')
        self.assertNotEqual(result.returncode, 0)

    def test_requires_parameters(self):
        result = self.run_cli('missing.jpg')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('required', result.stderr)

if __name__ == '__main__':
    unittest.main()
