#!/usr/bin/env python3
"""Generate time-limited unlock codes for Program Timer.

Usage:
  UNLOCK_CODE=your_secret python3 generate_code.py <YYMMDD> <HH>

Examples:
  UNLOCK_CODE=mysecret python3 generate_code.py 260626 14
  UNLOCK_CODE=mysecret python3 generate_code.py 260626 00
"""

import hashlib, os, sys

def main():
    secret = os.environ.get('UNLOCK_CODE')
    if not secret:
        print("Error: UNLOCK_CODE env var not set", file=sys.stderr)
        sys.exit(1)
    if len(sys.argv) < 3:
        print(f"Usage: UNLOCK_CODE=<secret> {sys.argv[0]} <YYMMDD> <HH>", file=sys.stderr)
        sys.exit(1)
    date = sys.argv[1]
    hour = sys.argv[2]
    if len(date) != 6 or not date.isdigit():
        print("Error: YYMMDD must be 6 digits", file=sys.stderr)
        sys.exit(1)
    if len(hour) != 2 or not hour.isdigit():
        print("Error: HH must be 2 digits", file=sys.stderr)
        sys.exit(1)
    data = f"{date}{hour}"
    h = hashlib.sha256((secret + data).encode()).hexdigest()[:16]
    print(f"TIMER-{date}-{hour}-{h}")

if __name__ == '__main__':
    main()
