#!/usr/bin/env python3
"""Common test client library copied for organization under tests/common."""
from pathlib import Path
import runpy

# Execute the original client located at tests/test_client.py
target = Path(__file__).resolve().parents[1].joinpath('test_client.py')
if __name__ == '__main__':
    runpy.run_path(str(target), run_name='__main__')
