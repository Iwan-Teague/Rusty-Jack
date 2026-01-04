#!/usr/bin/env python3
"""Proxy launcher that runs the common test client under tests/common.
This keeps compatibility for scripts that reference tests/test_client.py at the repo root.
"""
import runpy
from pathlib import Path

if __name__ == '__main__':
    repo_tests = Path(__file__).resolve().parents[0]
    target = repo_tests.joinpath('common', 'test_client.py')
    runpy.run_path(str(target), run_name='__main__')
