#!/usr/bin/env python3
import sys
try:
    import blake3
except ImportError:
    print('blake3 module required: pip install blake3', file=sys.stderr)
    sys.exit(1)

if len(sys.argv) != 3:
    print('usage: generate_slice.py <input> <output>', file=sys.stderr)
    sys.exit(1)
inp, out = sys.argv[1], sys.argv[2]
with open(inp, 'rb') as f:
    data = f.read()
with open(out, 'wb') as f:
    f.write(data)
print(blake3.blake3(data).hexdigest())
