#!/usr/bin/env python3
"""Generate 8-character unlock codes for Program Timer.

Code format: MM-DD-XXXX (e.g. "06-1a-a1b2")
  MM = month in hex (01-0C)
  DD = day in hex (01-1F)
  XXXX = HMAC-SHA256(secret + YYYYMMDD) first 4 hex chars

Usage:
  UNLOCK_CODE=your_secret python3 generate_code.py <YYYYMMDD>

Examples:
  UNLOCK_CODE=mysecret python3 generate_code.py 20260626
"""

import hashlib, os, sys

def main():
    secret = os.environ.get('UNLOCK_CODE')
    if not secret:
        print("Error: UNLOCK_CODE env var not set", file=sys.stderr)
        sys.exit(1)
    if len(sys.argv) < 2:
        print(f"Usage: UNLOCK_CODE=<secret> {sys.argv[0]} <YYYYMMDD>", file=sys.stderr)
        sys.exit(1)
    date = sys.argv[1]
    if len(date) != 8 or not date.isdigit():
        print("Error: YYYYMMDD must be 8 digits (e.g. 20260626)", file=sys.stderr)
        sys.exit(1)
    mm = int(date[4:6])
    dd = int(date[6:8])
    if mm < 1 or mm > 12 or dd < 1 or dd > 31:
        print("Error: invalid date", file=sys.stderr)
        sys.exit(1)
    mm_hex = f"{mm:02x}"
    dd_hex = f"{dd:02x}"
    h = hashlib.sha256((secret + date).encode()).hexdigest()[:4]
    print(f"{mm_hex}-{dd_hex}-{h}")

if __name__ == '__main__':
    main()
