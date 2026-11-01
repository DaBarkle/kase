#!/usr/bin/env python3
"""Watches Kase's log for the live case state and prints it big, for testing by hand.

    python3 tools/case-watch.py            # follow the running app
    python3 tools/case-watch.py --check    # also count misses: say what you did, see if it agrees

Kase logs every change it decides on as a line like

    case 11:22:33:44:55:66 #42 at 01234 ms: lid Open, L Seated, R Out (Aacp)

This tails that line from the Flatpak's log (or the plain one) and shows the lid and
each pod, with how long since the last change, so a person opening the case and pulling
pods out can see at once whether Kase saw it, and how fast.

With --check, type what you just did after each step (o = lid open, c = lid closed,
l/r = left/right out, L/R = left/right back in), and it scores whether the last state
Kase logged agrees. A summary prints on Ctrl+C.
"""

import argparse
import os
import re
import select
import sys
import time

LOGS = [
    "~/.var/app/io.github.dabarkle.Kase/.local/state/kase/kase.log",
    "~/.local/state/kase/kase.log",
]
LINE = re.compile(r"case (\S+) #(\d+) at (\d+) ms: lid (\w+), L (\w+), R (\w+) \((\w+)\)")
WORDS = {"Open": "OPEN", "Closed": "SHUT", "Seated": "in case", "Out": "OUT", "Unknown": "?"}


def find_log():
    for p in LOGS:
        p = os.path.expanduser(p)
        if os.path.exists(p):
            return p
    sys.exit("No Kase log found (is Kase running?). Looked in:\n  " + "\n  ".join(LOGS))


def follow(path):
    """Yields new lines, reopening the file when the app rotates it."""
    f = open(path, errors="replace")
    f.seek(0, os.SEEK_END)
    ino = os.fstat(f.fileno()).st_ino
    while True:
        line = f.readline()
        if line:
            yield line
            continue
        try:
            if os.stat(path).st_ino != ino:
                f = open(path, errors="replace")
                ino = os.fstat(f.fileno()).st_ino
                continue
        except FileNotFoundError:
            pass
        yield None


def show(state, since):
    lid, left, right, src, seq = state
    sys.stdout.write(
        f"\r\033[K  #{seq:<4} lid {WORDS.get(lid, lid):<5}  "
        f"L {WORDS.get(left, left):<8} R {WORDS.get(right, right):<8} "
        f"({src}, {since:4.1f} s ago)"
    )
    sys.stdout.flush()


def expect(key, want):
    """Updates the expected (lid, L, R) after a step typed by the tester."""
    lid, left, right = want
    return {
        "o": ("Open", left, right),
        "c": ("Closed", left, right),
        "l": (lid, "Out", right),
        "r": (lid, left, "Out"),
        "L": (lid, "Seated", right),
        "R": (lid, left, "Seated"),
    }.get(key, want)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--log", help="log file (default: the app's)")
    ap.add_argument("--check", action="store_true", help="score each step you type")
    a = ap.parse_args()
    path = a.log or find_log()
    print(f"Watching {path}\nOpen the case, take pods out, put them back. Ctrl+C to stop.")
    if a.check:
        print("After each step type o/c (lid), l/r (out), L/R (back in), then Enter.")
    state, changed = None, time.time()
    want, ok, bad = ("Closed", "Seated", "Seated"), 0, 0
    lines = follow(path)
    try:
        while True:
            line = next(lines)
            if line:
                m = LINE.search(line)
                if m:
                    _, seq, _, lid, left, right, src = m.groups()
                    state, changed = (lid, left, right, src, int(seq)), time.time()
                    print()
            if state:
                show(state, time.time() - changed)
            if a.check and select.select([sys.stdin], [], [], 0)[0]:
                for key in sys.stdin.readline().strip():
                    want = expect(key, want)
                # Give Kase a moment if the step was just done, then catch up.
                time.sleep(0.5)
                while (more := next(lines)) is not None:
                    if m := LINE.search(more):
                        _, seq, _, lid, left, right, src = m.groups()
                        state, changed = (lid, left, right, src, int(seq)), time.time()
                got = state[:3] if state else None
                if got == want:
                    ok += 1
                    print(f"\n  ok   {want}")
                else:
                    bad += 1
                    print(f"\n  MISS want {want}, Kase says {got}")
            if not line:
                time.sleep(0.05)
    except KeyboardInterrupt:
        print()
        if a.check:
            print(f"{ok} of {ok + bad} steps agreed.")


if __name__ == "__main__":
    main()
