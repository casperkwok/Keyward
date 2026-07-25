#!/usr/bin/env python3
"""Fail if a localization key the Swift code asks for is not defined.

A missing key does not crash and does not warn: SwiftUI renders the key itself,
so the user is shown `use.step2` where a sentence should be. That shipped once
here, and was only caught by looking at a screenshot.

Deliberately one-directional. Keys that appear defined-but-unused are *not* an
error: they are reached through expressions this script cannot see
(`Button(copied ? "action.copied" : "use.copy")` is one), and failing on them
would train people to ignore the check.
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SOURCES = ROOT / "apps/mac/Keyward"
LANGS = ["en", "zh-Hans"]

PATTERNS = [
    r'Loc\.t\(\s*"([^"]+)"',
    r'Text\(\s*"([a-z][A-Za-z0-9]*(?:\.[A-Za-z0-9]+)+(?: %@)*)"',
    r'Button\(\s*"([a-z][A-Za-z0-9]*(?:\.[A-Za-z0-9]+)+)"',
    r'LocalizedStringKey\(\s*"([^"]+)"',
]

used: set[str] = set()
for swift in SOURCES.glob("*.swift"):
    text = swift.read_text(encoding="utf-8")
    for pattern in PATTERNS:
        used |= set(re.findall(pattern, text))

status = 0
defined_per_lang = {}
for lang in LANGS:
    path = SOURCES / f"Resources/{lang}.lproj/Localizable.strings"
    defined = set(re.findall(r'^"([^"]+)"\s*=', path.read_text(encoding="utf-8"), re.M))
    defined_per_lang[lang] = defined
    for key in sorted(used - defined):
        print(f"{path}: missing key {key!r}", file=sys.stderr)
        status = 1

# A key present in one language and absent in another shows the raw key to
# exactly the users who read that language, which is the hardest kind to notice.
only_in_one = defined_per_lang[LANGS[0]] ^ defined_per_lang[LANGS[1]]
for key in sorted(only_in_one):
    where = LANGS[0] if key in defined_per_lang[LANGS[0]] else LANGS[1]
    print(f"key {key!r} is defined only in {where}", file=sys.stderr)
    status = 1

if status == 0:
    print(f"{len(used)} keys referenced, all defined in {', '.join(LANGS)}")
sys.exit(status)
