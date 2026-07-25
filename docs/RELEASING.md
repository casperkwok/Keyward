# Releasing

A release is one command:

```
git tag v0.1.1 && git push origin v0.1.1
```

`.github/workflows/release.yml` does the rest: sets both version numbers from the
tag, builds, signs, notarizes, publishes the GitHub Release, and appends to
`appcast.xml` so installed copies update themselves.

Nothing is built or signed on a normal push. Only tags matching `v*` release.

## The five secrets CI needs

Set these once, on the `Keyward` repository (Settings → Secrets and variables →
Actions). Four of them already exist on the `Beacon` repository and can be
copied across verbatim; the fifth is Keyward's own.

| Secret | What it is | Same as Beacon's? |
|---|---|---|
| `DEVELOPER_ID_CERT_P12` | Developer ID Application certificate + key, base64 of a `.p12` | yes |
| `P12_PASSWORD` | password for that `.p12` | yes |
| `AC_API_KEY_ID` | App Store Connect API key id | yes |
| `AC_API_ISSUER_ID` | App Store Connect issuer id | yes |
| `AC_API_KEY_P8` | that key's `.p8`, base64 | yes |
| `SPARKLE_ED_PRIVATE_KEY` | **Keyward's own** update-signing key | **no — see below** |

```
gh secret set SPARKLE_ED_PRIVATE_KEY -R casperkwok/Keyward \
  < ~/Developer/Projects/hobby/.keyward-updater-keys/keyward-sparkle.key
```

## The update-signing key

Keyward has its own EdDSA key rather than reusing the one Sparkle's tooling
keeps in the login keychain for all of an author's apps. Sparkle says one key
for everything is fine, and for most people it is; here it would mean one
leaked CI secret is enough to push an update to two products, one of which is a
credential manager.

- private half: `~/Developer/Projects/hobby/.keyward-updater-keys/keyward-sparkle.key`
  — mode 0600, **outside every git repository**, and the only copy besides the
  CI secret.
- public half: `SUPublicEDKey` in `apps/mac/project.yml`, baked into every build.

**Back it up.** Installed copies only accept updates signed by the matching
private key. Lose it and every existing install is stranded on its current
version with no way to reach it — a new key means a new public key, which only
takes effect in builds nobody has yet.

## Why the two version numbers must agree

The app's version comes from `MARKETING_VERSION` in `apps/mac/project.yml`; the
daemon's from `version` in the workspace `Cargo.toml`. On launch the app asks the
running daemon its version, and retires one that does not match — a Sparkle
update replaces the bundle but leaves the previous daemon running, and adopting
it would leave the process that holds the secrets on the old build.

The release job sets both from the tag, so they cannot drift. If they ever did,
the symptom is the daemon restarting on every single launch, dropping any open
broker sessions with it.

## Verifying a release did what it claims

The job fails rather than shipping if any of these do not hold, but the same
three commands work on a downloaded build:

```
codesign --verify --deep --strict --verbose=2 Keyward.app
spctl --assess --type execute --verbose=4 Keyward.app   # what a user's Mac asks
xcrun stapler validate Keyward.app                      # notarization is attached
```

`spctl` is the one that matters. A bundle can pass `codesign` and still be
refused on a user's machine.

## Signing locally

`./sign.sh <path to Keyward.app>` — the same script CI runs, deliberately, so
the two cannot drift. It signs inside out, including the Sparkle framework and
its XPC services, and then checks that every nested piece carries the app's team
identifier. That check exists because a build once passed
`codesign --verify --deep --strict` and still died at launch with "different
Team IDs" — `--deep` does not look at that.

A local build is signed but **not notarized**, which is fine on the machine that
built it and not fine anywhere else.
