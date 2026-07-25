# Keyward

**[keyward.casperkwok.dev](https://casperkwok.github.io/keyward-site/)** · [Download](https://github.com/casperkwok/Keyward/releases/latest) · [Releasing](docs/RELEASING.md)

**A local secret broker for the AI-coding era.** Your project's API keys live in
the OS keychain. Your coding agent gets a reference, a scoped loopback session, or
nothing at all — never the literal value.

> **Status: pre-release, unannounced.** The core, the broker and the CLI work; the
> Mac app is most of the way through its first milestone and is not signed,
> notarized or installable as a product. See [Current state](#current-state) — it
> is deliberately blunt about what does not exist.

---

## Why this and not `op run`

Every mature secrets tool — 1Password's `op run`, Doppler, Infisical — gives you
two things: a reference you can commit (`op://vault/item/field`) and injection at
launch, where the real value is substituted into the child process's environment.
That is genuinely useful and it fixes leaked `.env` files, leaked commits and
leaked backups. **It does not fix the problem this product exists for**, because
when the thing reading your filesystem is an LLM coding agent, the agent is very
often the *parent of* the process receiving the injected value — and `op run --
env` prints the secret. An agent debugging a 401 will reach for exactly that.

Keyward's answer is a third mode nobody else offers on a single developer's
machine: **the broker**. The child is pointed at `http://127.0.0.1:PORT` and given
a session token; the daemon attaches the real `Authorization` header on the way
upstream and streams the response back. `env` shows the agent a token that is
worthless off this machine and dies with the session. There is no value in the
child's address space to print, inline into a test, or ingest into a transcript.
That is the whole argument, and everything else in the repo is in service of it.

The full reasoning is in [`ARCHITECTURE.md`](ARCHITECTURE.md) §4 ("Why T1 is worth
building") and §6.1. The threat model, including what Keyward explicitly **cannot**
defend against, is §3 — read it before trusting anything here with a real key.

---

## How it works, in four sentences

1. Values go into the OS keychain. `keywardd` is the only process that ever holds
   plaintext.
2. Your `.env` holds `keyward://stripe`, which is a name, not a secret, and is safe
   to commit.
3. `kw exec -- npm run dev` resolves each reference: brokered where it can be,
   handed over where it cannot, and it tells you which it did for each variable.
4. Your agent reads a `CLAUDE.md` / `AGENTS.md` block — generated from your live
   vault by the Mac app — that tells it references are working config and never to
   replace one with a real key.

---

## Repository layout

| Path | What it is |
|---|---|
| `crates/keyward-core` | Vault model, `keyward://` grammar, policy engine. **No I/O, and no type that represents a plaintext value.** Everything else is a frontend to the decisions made here. |
| `crates/keywardd` | The daemon. Keychain, IPC server, peer attestation, approval prompts, and the loopback broker. The only holder of plaintext. |
| `crates/kw` | The CLI. A thin IPC client with no privileges: `list`, `ref`, `exec`, `render`, `scan`, `mcp`, and the add/rotate/rm verbs. |
| `crates/keywardd-stub` | A fake daemon answering the same wire protocol from canned data, so the desktop app can be built and screenshotted with no keychain and no secrets. |
| `apps/mac` | The SwiftUI app. Links no Rust, declares no bridging header — it speaks newline-delimited JSON over a Unix socket and nothing else. |
| `ARCHITECTURE.md` | The spec: threat model, tier model, data model, IPC protocol, milestones, and what was cut. |
| `DESIGN.md` | The design system: palette, type, components, and the decisions that were reversed. |
| `docs/USE_CASES.md` | Who this is for and which scenario justifies which feature. |

**There is no FFI anywhere.** The Swift app links no Rust; the Rust daemon exports
no C ABI. The entire cross-language contract is one line of JSON per request. That
keeps the daemon a real security boundary rather than a code-organization
convention — and it is why the stub above can exist.

---

## Build and run

Requires **Rust 1.85+** (edition 2024) and, for the app, **Xcode 16+** and
macOS 14+.

### The Rust side

```bash
cargo build --workspace          # all four crates
cargo test  --workspace          # 74 tests; no keychain, no socket, no GUI needed
cargo clippy --workspace --all-targets
```

The workspace `forbid`s `unsafe_code` and `deny`s `unwrap`, `expect`, `panic` and
`indexing_slicing`. That last group is not style: a daemon that panics on a request
path denies service to every other session it is serving.

**Run the daemon** — it creates `~/Library/Application Support/Keyward/`, binds a
socket there, and binds the broker on an ephemeral loopback port:

```bash
cargo run -p keywardd
# keywardd listening on /Users/you/Library/Application Support/Keyward/keywardd.sock
# broker on http://127.0.0.1:51823
```

**Use the CLI** in another terminal:

```bash
cargo run -p kw -- status
cargo run -p kw -- add stripe        # prompts on a TTY; never takes a value in argv
cargo run -p kw -- list
cargo run -p kw -- ref stripe        # keyward://stripe

echo 'STRIPE_SECRET_KEY=keyward://stripe' > .env
cargo run -p kw -- exec -- printenv STRIPE_SECRET_KEY
```

For day-to-day use put the binary on your path: `cargo install --path crates/kw`,
or `ln -s "$PWD/target/debug/kw" /usr/local/bin/kw`. (The shipping plan is to
bundle it inside the app — see [Current state](#current-state); that is not built.)

**Run the stub instead of the daemon** when you want the UI without a keychain.
Stop `keywardd` first — they bind the same socket:

```bash
cargo run -p keywardd-stub
```

It serves five canned secrets with live-looking timestamps, and deliberately
**refuses** `secret.hand`, `broker.open` and `scrub.values` with `not_implemented`,
so a bug in the app cannot hide behind a stub that pretends to broker.

### The Mac app

The Xcode project is generated from `apps/mac/project.yml` and also committed, so
either path works:

```bash
# with XcodeGen (brew install xcodegen) — regenerate after adding files
cd apps/mac && xcodegen generate

# build and run
xcodebuild -project apps/mac/Keyward.xcodeproj -scheme Keyward -configuration Debug build
open apps/mac/Keyward.xcodeproj      # or just work in Xcode
```

The app is unsigned for local development (`CODE_SIGN_IDENTITY: "-"`), not
sandboxed — the sandbox forbids the loopback broker and the `PATH` symlink — and
expects a daemon or stub already listening on the socket. It does **not** start one.

**Render every screen to PNG** without driving the live UI, for design review:

```bash
./path/to/Keyward.app/Contents/MacOS/Keyward --snapshot /tmp/shots
```

Light and dark × English and 中文, from fixtures rather than from whatever the
daemon happened to return that second.

### Where things live at runtime

| Path | Contents |
|---|---|
| `~/Library/Application Support/Keyward/vault.json` | Metadata, mode 0600. Human-readable and contains **zero secrets** — back it up, commit it, sync it. |
| `~/Library/Application Support/Keyward/uses.jsonl` | Append-only usage log, mode 0600. |
| `~/Library/Application Support/Keyward/keywardd.sock` | IPC socket, mode 0600. |
| macOS keychain, service `ai.keyward.vault` | The values. One item per (name, revision). Inspect them in Keychain Access under that one heading. |

---

## Current state

Honest version. Per-milestone detail with file references is in
[`ARCHITECTURE.md`](ARCHITECTURE.md) §11.4.

**Works:**

- The core model, reference grammar and policy engine, with tests asserting the
  headline guarantee — no approval, setting or caller can move a brokered value out
  of the daemon.
- The daemon: keychain storage, atomic metadata writes, the usage log, the IPC
  protocol, and peer attestation on macOS (the log records what the *kernel* said
  the caller was, separately from what the caller claimed).
- The broker: loopback proxy, session tokens, streaming passthrough, TTL and
  reaper, `Host` / `Origin` / `Referer` checks, live request counts.
- Approval prompts on the daemon side — a request parks, a UI is meant to answer,
  and every path that is not an explicit allow fails closed.
- The CLI: `list`, `ref`, `status`, `add`, `rotate`, `rm`, `exec`, `render`,
  `scan`, `mcp`, plus output scrubbing that survives a value split across two
  `write()` calls.
- The Mac app: list and detail, menu-bar popover, add/rename/replace/delete, the
  usage table and activity strip, the live-session card, the "For your AI"
  instructions sheet, settings, and full English/中文 localisation.

**Does not work, or does not exist:**

- **Nothing can mark a secret as brokered.** `vault.add` accepts a delivery mode,
  but neither `kw add` nor the app's add sheet sends one, so every secret created
  through a frontend is handed over. The differentiator is built and currently
  unreachable without editing `vault.json` by hand. This is the top of the list.
- **No UI answers an approval prompt**, so a secret set to "ask" parks for 60
  seconds and times out.
- **Nothing manages the daemon.** The login item registers the app; `keywardd` is
  started by you, from a terminal. `kw` is not bundled or offered on `PATH`.
- **Nothing edits your project files.** No drag-a-folder import, no `.env`
  rewriting, no `.gitignore` check, no `.mcp.json` rewrite, no "Run with Keyward".
  You paste the reference and you paste the agent instructions.
- **The broker always sends `Authorization: Bearer` upstream**, so an API that
  wants `x-api-key` (Anthropic's own) is not brokerable yet.
- **No Windows app, no Linux paths, no signing, no notarization, no updater, no
  licensing, and no `LICENSE.md`** — despite `Cargo.toml` pointing at one.
- The usage log never rotates; the 1 MiB IPC frame cap is specified and not
  enforced; broker connections are not peer-verified.

---

## What this is not

- Not a team secrets manager. No server, no sync, no accounts, no cloud.
- Not a password manager. Machine-to-machine credentials only.
- Not a replacement for cloud IAM. If your provider issues short-lived scoped
  tokens, use those; Keyward is for the long-lived static keys most AI providers
  still hand out.
- Not an attempt to sandbox your agent. A process running as you can debug the
  daemon or drive the UI, and no userland tool can win that fight
  (`ARCHITECTURE.md` §3.2, adversary A5).

---

## Licence

**FSL-1.1-ALv2** (`LICENSE.md`) — source-available, converting to Apache-2.0 after
two years. The reasoning, and why open source is a *sales argument* rather than a
concession for a tool that asks to hold your API keys, is in `ARCHITECTURE.md`
§13.1–13.2.

The licence file itself has not been committed yet. Until it is, treat this as
"all rights reserved, read the source": `Cargo.toml` references `LICENSE.md` and
that file does not exist.
