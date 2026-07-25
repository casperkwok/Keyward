#!/bin/bash
# Sign the app, both bundled binaries, and the Sparkle framework, with one
# identity.
#
# Ad-hoc signatures have no authority, so macOS cannot tell one build from the
# next — and a keychain ACL bound to "this executable" stops matching the moment
# anything is rebuilt or copied. That is what makes the daemon prompt for the
# login keychain password on every run.
#
# CI runs this same script rather than keeping its own copy of the commands. The
# two drifting apart is not hypothetical: the first Sparkle build here passed
# `codesign --verify --deep --strict` and still died at launch with "different
# Team IDs", because the framework Xcode had already signed was never re-signed
# and `--deep` does not check that.
set -euo pipefail

APP="${1:?usage: sign.sh <path to Keyward.app>}"
ID="${CODESIGN_IDENTITY:-Developer ID Application: Pixel Bit Network Co., Ltd (WB7GQVWQ5S)}"
ENTITLEMENTS="$(cd "$(dirname "$0")" && pwd)/apps/mac/Keyward/Keyward.entitlements"

sign() {
  codesign --force --options runtime --timestamp --sign "$ID" "$@"
}

# Inside out. A bundle's signature covers its contents, so signing the outer app
# first would leave a seal over bytes that are about to change.
FW="$APP/Contents/Frameworks/Sparkle.framework"
if [ -d "$FW" ]; then
  # Sparkle ships XPC services and two helper bundles, each independently loaded
  # at runtime and each independently checked.
  while IFS= read -r -d '' xpc; do sign "$xpc"; done \
    < <(find "$FW" -name "*.xpc" -print0)
  [ -e "$FW/Versions/Current/Autoupdate" ] && sign "$FW/Versions/Current/Autoupdate"
  [ -e "$FW/Versions/Current/Updater.app" ] && sign "$FW/Versions/Current/Updater.app"
  sign "$FW"
fi

for BIN in keywardd kw; do
  sign "$APP/Contents/MacOS/$BIN"
done

codesign --force --options runtime --timestamp \
  --entitlements "$ENTITLEMENTS" \
  --sign "$ID" "$APP"

codesign --verify --deep --strict --verbose=2 "$APP"

# `--deep` verifies structure, not that every piece shares this app's team — and
# a team mismatch is exactly what stops the app launching, with no crash report
# and no non-zero exit anywhere. Check it directly.
TEAM="$(codesign -dv --verbose=2 "$APP" 2>&1 | sed -n 's/^TeamIdentifier=//p')"
STATUS=0
while IFS= read -r item; do
  [ -e "$item" ] || continue
  ITEM_TEAM="$(codesign -dv --verbose=2 "$item" 2>&1 | sed -n 's/^TeamIdentifier=//p')"
  if [ "$ITEM_TEAM" != "$TEAM" ]; then
    echo "MISMATCH: $item has team '${ITEM_TEAM:-none}', app has '$TEAM'" >&2
    STATUS=1
  fi
done < <(
  find "$APP/Contents" -type d \( -name "*.framework" -o -name "*.xpc" -o -name "*.app" \)
  printf '%s\n' "$APP/Contents/MacOS/keywardd" "$APP/Contents/MacOS/kw"
)
[ "$STATUS" -eq 0 ] || {
  echo "refusing to report success: nested code would fail to load" >&2
  exit 1
}

echo
echo "all nested code signed by team $TEAM"
codesign -dv --entitlements - "$APP" 2>&1 | grep -E "Authority|TeamIdentifier|Identifier="
