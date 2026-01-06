#!/usr/bin/env python3
import re

with open('/Users/ianreitsma/projects/the-block/test-logs/full-20260106-084542_linux.log', 'r') as f:
    content = f.read()

# Find test failures
failure_pattern = r'test (\S+) \.\.\. FAILED'
failures = re.findall(failure_pattern, content)

print("=== FAILED TESTS ===")
for f in failures:
    print(f)

# Find the partition_heals_to_majority test specifically
if 'partition_heals_to_majority' in content:
    print("\n=== Found partition_heals_to_majority in log ===")
    idx = content.find('partition_heals_to_majority')
    print(content[max(0, idx-500):idx+2000])

if 'kill_node_recovers' in content:
    print("\n=== Found kill_node_recovers in log ===")
    idx = content.find('kill_node_recovers')
    print(content[max(0, idx-500):idx+2000])

# Look for any FAIL or panic
panic_pattern = r'thread.*panicked at.*\n.*\n.*'
panics = re.findall(panic_pattern, content)
if panics:
    print("\n=== PANICS ===")
    for i, p in enumerate(panics[:5]):
        print(f"\nPanic {i+1}:")
        print(p)
