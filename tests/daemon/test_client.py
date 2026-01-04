#!/usr/bin/env python3
"""Proxy launcher that runs the top-level test_client.py from the tests root.
This makes the daemon test runner script use a consistent entrypoint.
"""
import runpy
from pathlib import Path

if __name__ == '__main__':
    repo_tests = Path(__file__).resolve().parents[1]
    target = repo_tests.joinpath('test_client.py')
    runpy.run_path(str(target), run_name='__main__')
