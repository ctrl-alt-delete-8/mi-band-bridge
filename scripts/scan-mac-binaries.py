#!/usr/bin/env python3
"""Scan Mac binaries for private details before they go out.

Run it on a Mac, with files or folders:

    python3 scripts/scan-mac-binaries.py <file or folder> ...

scripts/package-mac.sh runs it over both app bundles, before it signs them.
It reads each Mach-O file as bytes, and as UTF-16 text at both byte offsets,
and exits 1 when one holds any of these:

  - a path in a Mac home folder: /Users/<anyone>/
  - a Gmail address
  - the name of the Mac user who builds, in its usual spellings
  - the MAC address of the band this Mac pairs with, in its usual forms

The last two come from this Mac, not from this file, so the script names no
person, and can go public. patterns() holds every pattern, and nothing else
does. A hit is reported by what it is and how many times, never by the text
it found. scripts/scan-windows-binaries.ps1 does the same on Windows.
"""

import getpass
import os
import re
import subprocess
import sys

MACH_O = {
    b"\xfe\xed\xfa\xce", b"\xfe\xed\xfa\xcf", b"\xce\xfa\xed\xfe",
    b"\xcf\xfa\xed\xfe", b"\xca\xfe\xba\xbe", b"\xbe\xba\xfe\xca",
}


def full_name():
    """The Mac account's full name, or nothing."""
    for command in (["id", "-F"], ["dscl", ".", "-read", "/Users/" + getpass.getuser(), "RealName"]):
        try:
            said = subprocess.run(command, capture_output=True, text=True).stdout
        except OSError:
            continue
        name = said.replace("RealName:", "").strip()
        if name:
            return name
    return ""


def patterns():
    """Every pattern the scan uses, by what it finds."""
    found = {
        "a path in a Mac home folder": r"/users/[^/\x00-\x1f]{1,64}/",
        "a Gmail address": r"[a-z0-9._%+-]+@gmail\.com",
    }

    # The Mac user who builds: the account's full name, and the name of its
    # home folder. Each word of three letters or more alone, and the whole
    # name with each usual separator, or none. A bundle identifier such as
    # com.<name>.app may hold the home folder's name, by the maintainer's
    # choice, so a name right after "com." is allowed. So are the letters of a
    # name inside a longer word, which is another word.
    names = {full_name(), os.path.basename(os.path.expanduser("~"))}
    spellings = set()
    for name in filter(None, names):
        parts = [part for part in re.split(r"[\s._-]+", name) if part]
        spellings.update(part for part in parts if len(part) >= 3)
        if len(parts) > 1:
            for join in (" ", "", ".", "_", "-"):
                spellings.add(join.join(parts))
    if spellings:
        found["the name of the user who builds"] = "|".join(
            r"(?<![a-z0-9])(?<!com\.)" + re.escape(spelling) for spelling in sorted(spellings)
        )

    # The band this Mac pairs with: [device] mac in Mi Band Bridge's config,
    # with colons, dashes or nothing between its bytes, and as an EUI-64.
    config = os.path.expanduser("~/Library/Application Support/BandBridge/config.toml")
    if os.path.exists(config):
        with open(config, encoding="utf-8", errors="replace") as f:
            line = re.search(r'^\s*mac\s*=\s*"([0-9A-Fa-f:-]+)"', f.read(), re.M)
        if line:
            digits = re.sub(r"[:-]", "", line.group(1)).lower()
            if re.fullmatch(r"[0-9a-f]{12}", digits):
                pairs = [digits[i : i + 2] for i in range(0, 12, 2)]
                found["the MAC address of the band this Mac pairs with"] = (
                    "[:-]?".join(pairs) + "|" + digits[:6] + "fffe" + digits[6:]
                )
    return found


def is_mach_o(path):
    with open(path, "rb") as f:
        return f.read(4) in MACH_O


def main(paths):
    files = []
    for item in paths:
        if os.path.isdir(item):
            for folder, _, names in os.walk(item):
                for name in sorted(names):
                    path = os.path.join(folder, name)
                    if os.path.isfile(path) and not os.path.islink(path) and is_mach_o(path):
                        files.append(path)
        elif os.path.isfile(item):
            files.append(item)
        else:
            print(f"There is nothing to scan at {item}.", file=sys.stderr)
            return 1
    if not files:
        print(f"There is no Mach-O file to scan in {', '.join(paths)}.", file=sys.stderr)
        return 1

    found = patterns()
    print(f"Scanning {len(files)} files for: {'; '.join(found)}.")
    compiled = {label: re.compile(p, re.IGNORECASE) for label, p in found.items()}
    hits = 0
    for path in files:
        with open(path, "rb") as f:
            data = f.read()
        # UTF-8 and ASCII text through Latin-1, which keeps one character a
        # byte, and UTF-16 text at both byte offsets.
        texts = [data.decode("latin-1")]
        for start in (0, 1):
            body = data[start:]
            texts.append(body[: len(body) // 2 * 2].decode("utf-16-le", errors="replace"))
        for label, pattern in compiled.items():
            count = sum(len(pattern.findall(text)) for text in texts)
            if count:
                print(f"  HIT {path}: {label}, {count} times")
                hits += count
    print(f"Scanned {len(files)} files: {hits} hits.")
    return 1 if hits else 0


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print(__doc__.strip().splitlines()[0], file=sys.stderr)
        print("usage: python3 scripts/scan-mac-binaries.py <file or folder> ...", file=sys.stderr)
        sys.exit(2)
    sys.exit(main(sys.argv[1:]))
