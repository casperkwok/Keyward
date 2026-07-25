#!/usr/bin/env python3
"""Insert a new <item> into appcast.xml for a release.

Usage: update_appcast.py <version> <download_url> '<sign_update output>'
where the sign_update output looks like: sparkle:edSignature="..." length="123"
"""
import sys
import os
import re
import datetime

version, url, siginfo = sys.argv[1], sys.argv[2], sys.argv[3]

m = re.search(r'sparkle:edSignature="([^"]+)"\s+length="([0-9]+)"', siginfo)
if not m:
    sys.exit(f"could not parse sign_update output: {siginfo!r}")
signature, length = m.group(1), m.group(2)

pub_date = datetime.datetime.now(datetime.timezone.utc).strftime("%a, %d %b %Y %H:%M:%S +0000")

item = f"""    <item>
      <title>{version}</title>
      <sparkle:version>{version}</sparkle:version>
      <sparkle:shortVersionString>{version}</sparkle:shortVersionString>
      <sparkle:minimumSystemVersion>14.0</sparkle:minimumSystemVersion>
      <pubDate>{pub_date}</pubDate>
      <enclosure url="{url}" sparkle:edSignature="{signature}" length="{length}" type="application/octet-stream"/>
    </item>"""

path = "appcast.xml"
if os.path.exists(path):
    xml = open(path, encoding="utf-8").read()
else:
    sys.exit("appcast.xml is missing; it is committed to the repository, not generated")

if f"<sparkle:shortVersionString>{version}</sparkle:shortVersionString>" in xml:
    print(f"appcast already contains {version}")
    sys.exit(0)

marker = "    <title>Keyward</title>\n"
if marker not in xml:
    sys.exit("appcast.xml has no <title>Keyward</title> to insert after")
xml = xml.replace(marker, marker + item + "\n", 1)
open(path, "w", encoding="utf-8").write(xml)
print(f"appcast updated for {version}")
