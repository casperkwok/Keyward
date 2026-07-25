# Keyward — Design Document

> A local secret broker for the AI-coding era. Your keys live in the OS keychain;
> your AI agent gets a reference, a scoped session, or nothing at all — never the
> literal value.

**Status:** Draft v0.2 · **License:** FSL-1.1-ALv2 — text in `LICENSE.md` (see §13.2)
**Shape:** A paid consumer desktop app with a fully open codebase. Not a package,
not a library, not a `cargo install`. See §13 for what that implies.
**Targets:** macOS 14+ (Swift/SwiftUI), Windows 10+ (Tauri 2/Rust), bundled CLI (Rust)

---

## 1. Problem

**Scope, stated first because it is easy to drift from.** Keyward protects the
secrets *inside the project a coding agent is working on* — the Stripe key, the
OpenAI key the app calls at runtime, the database URL, the webhook secret. It is
**not** a tool for configuring which model provider Claude Code or Codex talks to.
That is a different product (see `beacon-mac/` next door), and every feature that
drifts toward it should be cut.

Restated as a sentence: **the AI is the threat model, not the client.**

You are building something with Claude Code or Codex. The agent has a shell, reads
your files, writes your config, and runs your dev server. Your project's API keys
are sitting in `.env`. So:

- the agent reads `.env` while debugging, and the literal key enters a context
  window that is uploaded to a model vendor and retained under its policy
- the agent inlines a key into source to make a failing test pass, and you approve
  the diff because 40 lines of changes with one high-entropy string reads as noise
- the key was pasted into a terminal to begin with, so it is in `~/.zsh_history`
- someone commits `.env`, or pastes it into an issue, or shares a screen

None of this requires anyone to act maliciously. The agent is not an attacker; it
is careless in exactly the way a fast, capable, context-hungry process is careless.

That is the distinguishing constraint versus a password manager: **the adversary is
also your assistant.** Storing secrets safely at rest does nothing about it. The
value has to stay out of the agent's *reach*, not merely off its disk.

## 1.5 How it is actually used

The mechanism sections below describe what Keyward *can* do. This section describes
what a person does, and — the question everything hinges on — **how the coding agent
learns that it must go through Keyward.**

### The failure mode that decides the product

The agent runs commands constantly. If it runs `npm run dev` directly, the app
receives the literal string `keyward://stripe`, authentication fails, and the agent
does what a capable agent does: it debugs. It finds a reference where a key should
be, concludes the config is broken, and **puts a real key back** — asking the user
to paste one, or pulling one out of shell history.

That single behaviour inverts the entire product. Every design decision here exists
to prevent it, and it is not preventable by making the mechanism better. It is only
preventable by **telling the agent**.

### Keyward writes the agent's instructions

The block goes into the project's `CLAUDE.md` or `AGENTS.md` — whichever the
project already has. This is the standard channel by which coding agents receive
project rules, it needs no cooperation from any vendor, and both Claude Code and
Codex read it on every run.

```markdown
## Secrets

This project's secrets are managed by Keyward. `.env` holds `keyward://`
references, not real values.

- Run anything that needs a secret through `kw exec`:
  `kw exec -- npm run dev`, `kw exec -- pytest`.
- `keyward://…` is a working reference, **not** a placeholder or a TODO. Never
  replace one with a literal value, never ask the user to paste a key, and never
  read a key out of shell history.
- A 401/403 next to a `keyward://…` value means the command ran without the
  wrapper. Re-run it with `kw exec` — do not change the config.
- `kw list` shows which secrets exist. It returns names only, never values.

Available in this project:

- `keyward://stripe` — Stripe
- `keyward://pg-prod` — 生产数据库
```

Those four bullets are ordered by what an agent needs at the moment it goes wrong.
The second one is load-bearing and is phrased as a prohibition with its three most
likely evasions named, because "don't hardcode secrets" alone is a principle an
agent will happily reason its way around when a test is failing.

**The Mac app generates this block from the live vault** — the "For your AI"
button in the toolbar opens a sheet that renders it and copies it to the
clipboard. The reference list at the bottom is the interesting half: it is built
from `vault.list`, so the agent is handed the names that actually exist rather
than a syntax it has to guess at, and an empty vault says so ("none stored yet")
instead of shipping an example the agent might treat as real. The block is
localised with the rest of the UI, because the agent reads whatever the user's
project is written in.

**It is generated, not written.** An earlier plan had Keyward append the section
to `CLAUDE.md` itself, on import, showing a diff first. That is still the right
end state — see §10.5 — but nothing in the product edits a user's files yet, and a
"Keyward writes your instructions" claim with a copy button behind it would be the
kind of overstatement §3 exists to prevent. Today the user pastes it. The sheet
says where.

### The three-step setup, once

1. **Add secrets** — name and value, or (not yet built) drag a project folder in
   and let Keyward find them (§10.5).
2. **Keyward rewrites** — values into the keychain, `.env` and `.mcp.json` into
   references, `.gitignore` checked. *Not yet built; today the user edits `.env`
   themselves, and the add sheet shows the exact line to paste.*
3. **Keyward writes the instructions** — the block above. *Today: generated and
   copied from the "For your AI" sheet; the user pastes it.*

The target is that the user reviews one diff and clicks once. They should not have
to learn a reference syntax, and should never type a command to make it work. Two
of the three steps are still manual, which is the single largest gap between this
section and the shipping app.

### Daily use, ideally zero-effort

| Who runs it | How | Effort | Built? |
|---|---|---|---|
| The agent | Learns it from the MCP tool descriptions, prefixes with `kw exec` | none | yes |
| The user, manually | `kw exec -- npm run dev` | one prefix | yes |

There is no shell hook and there will not be one. An earlier draft planned a
`direnv`-style `kw shell-init` that would activate on `cd`, for the case of a
person running their own project in a terminal. That case turned out not to be
the product's case: the people this is built for do not run their projects, they
ask an agent to. A hook installed into someone's shell to serve a scenario they
never enter is intrusion with no benefit.

The GUI has no "Run" button either, for the same reason.

### Being honest about the weak link

**This depends on the agent following instructions.** Usually it does. Not
always — and a design that pretends otherwise is the kind of security tool this
document keeps arguing against.

The instructions live in the MCP tool descriptions rather than in `CLAUDE.md`,
which is a real difference: the model reads a tool description on every call and
cannot edit it, whereas `CLAUDE.md` is a file in the repository that it can ignore,
rewrite, or never load. That narrows the gap. It does not close it.

So there are two backstops, neither of which depends on the agent's cooperation:

- `kw scan --staged` as a pre-commit hook rejects a literal key, whatever put it
  there.
- Keyward notices when a project's `.env` gains a value where a reference used to
  be, and says so in the app. The agent may undo the protection; it should not be
  able to do so quietly.

And one that removes the question entirely for HTTP credentials: a brokered secret
is never given to the agent's process at all, so there is nothing for a
disobedient agent to leak. That is why brokering is the default whenever it is
possible (§4).

---

## 2. Product shape

A background daemon that owns the secrets, plus three thin frontends:

| Component | Tech | Role |
|---|---|---|
| `keywardd` | Rust | The only process that ever holds plaintext. Owns keychain access, policy, audit log, broker proxy, IPC server. |
| Keyward.app (macOS) | Swift / SwiftUI, menu-bar | Human UI: add/edit/organize secrets, approve requests, read the audit log. |
| Keyward.exe (Windows) | Tauri 2 + React, tray | Same UI, same daemon protocol. |
| `kw` | Rust | The agent-and-terminal-facing surface. Injection, reference resolution, listing, brokering. |

**Everything ships in one installer.** The daemon lives inside the app bundle
(`Keyward.app/Contents/MacOS/keywardd`, managed as an `SMAppService` login item) and
the CLI lives beside it (`Keyward.app/Contents/MacOS/kw`). On first launch the app
offers to put `kw` on `PATH` — a symlink into `/usr/local/bin` on macOS, a `PATH`
entry on Windows — with a one-line explanation of what that does. There is no
`brew install`, no `cargo install`, no npm package. A user downloads one file,
double-clicks it, and has a working system.

The CLI never touches the keychain directly; it is a thin IPC client. That
centralizes policy enforcement and audit in one place, and it means the CLI binary
holds no secrets and needs no privileges of its own.

### 2.1 Technology stack

**Shared core — Rust, one Cargo workspace.** Everything that touches a secret is
Rust, built once, and compiled into both platforms' shipping artifacts:

| Crate | Contents | Key dependencies |
|---|---|---|
| `keyward-core` | Vault model, reference grammar, policy engine, usage-log types. **No I/O at all and no type representing plaintext.** | `serde`, `thiserror` |
| `keywardd` | Keychain adapter, IPC server, peer attestation, broker proxy, `vault.json` and the usage log. The only holder of plaintext. | `keyring`, `ureq` (rustls), `zeroize`, `nix` + `libproc` (macOS) |
| `kw` | CLI. Thin IPC client, no privileges, no secrets at rest. Output scrubber. | `serde_json`, `zeroize` |
| `keywardd-stub` | A fake daemon answering the §7 protocol from canned data. Holds no secrets, touches no keychain, opens no network socket. | `serde_json` |

`zeroize` on every buffer that has held a plaintext value; `rustls` (via `ureq`'s
feature flag) rather than system TLS so the broker's upstream trust store is
explicit and auditable.

**The workspace `forbid`s `unsafe_code`, and `deny`s `unwrap`, `expect`, `panic`
and `indexing_slicing`.** The last four are not style: a secrets daemon that
panics on a request path denies service to every other session it was serving, so
the fallible paths are written as `let … else` and `match` rather than as
assertions. `forbid` (not `deny`) on `unsafe` is deliberate — no inner `#[allow]`
can lift it — which is why peer attestation borrows `nix` and `libproc` for its
two syscalls instead of writing the FFI in-tree.

**Dependencies this document previously named and the code does not use:**
`tokio`, `hyper`/`reqwest`, `clap`, `rmcp`. The daemon and the broker are
thread-per-connection blocking I/O on the standard library, which for a loopback
proxy serving one machine's traffic is enough and is a great deal less to audit;
`ureq` is a blocking client, so there is no runtime anywhere. `kw` parses its own
argv in about forty lines rather than taking `clap` — the command surface is seven
verbs and the value-never-in-argv rule (§9) is easier to guarantee when nothing is
doing the parsing for you. `rmcp` returns if and when `kw mcp` is built (§10.4).

**`keywardd-stub` is not in the shipping product.** It exists so the desktop apps
can be built and snapshotted against the real wire format without a keychain, a
daemon, or any secret at all — which is the payoff of §2.1's decision to make the
cross-language contract a line of JSON rather than an FFI boundary: the thing on
the other end can be two hundred lines of anything. It deliberately refuses
`secret.hand`, `broker.open` and `scrub.values` with `not_implemented`, because a
stub that pretends to broker would let a bug in the app go unnoticed until the
real daemon replaced it.

**Frontends — native per platform.**

| | macOS | Windows |
|---|---|---|
| UI | Swift 6 + SwiftUI, `MenuBarExtra` | Tauri 2 + React 19 + TypeScript, tray + webview |
| Language boundary | None — talks IPC | Rust, links `keyward-core` directly |
| Daemon lifecycle | `SMAppService` login item | Child process of the tray app + Startup entry |
| Biometric unlock | `LocalAuthentication` (Touch ID) | `Windows.Security.Credentials.UI` (Hello) |
| Build | Xcode + XcodeGen (`project.yml`) | `pnpm` + `cargo tauri` |
| Updates | Sparkle 2, signed appcast | Tauri updater, signed manifest |
| Signing | Developer ID + notarization + staple | Authenticode |

**There is no FFI anywhere in this design.** The Swift app links no Rust and
declares no C bridging header; the Rust daemon exports no C ABI. The entire
cross-language contract is §7's newline-delimited JSON over a Unix socket. This is
the single most important structural decision in the stack: UniFFI / cbindgen /
`swift-bridge` all work, but each one couples the two build systems, breaks
independently on toolchain upgrades, and — worse here — would mean plaintext
crossing a language boundary inside one address space. A socket keeps the daemon a
genuine security boundary rather than a code-organization convention.

**Why not Tauri on macOS too?** It would halve the frontend work; Windows and macOS
would share one React codebase, and the daemon could be linked in-process. The
argument against is that macOS is the primary market for this product, Mac buyers
notice a webview menu bar, and the things that make the app feel trustworthy —
a real `MenuBarExtra`, native approval sheets, Touch ID, a 6 MB bundle instead of a
webview runtime — are exactly the ones Tauri makes awkward. For a paid consumer app
whose whole pitch is "trust me with your keys," that polish is load-bearing rather
than cosmetic. The cost is a duplicated UI layer, roughly 3–4k lines each, which is
acceptable because all the logic sits below the IPC line. Revisit only if Windows
turns out to be the larger market.

### Non-goals

- Not a team secrets manager. No server, no sync, no sharing. (Sync is a possible
  v2 via user-supplied storage; it is explicitly out of scope for v1 because it
  changes the threat model completely.)
- Not a password manager for browsers/logins. Machine-to-machine credentials only.
- Not a replacement for cloud IAM. If your provider supports short-lived scoped
  tokens, use those; Keyward is for the long-lived static keys that most AI
  providers still hand out.
- Not an attempt to sandbox the AI agent. See §3.

---

## 3. Threat model

State this plainly, because a secrets tool that overclaims is worse than none.

### 3.1 Assets

Long-lived static credentials: LLM provider API keys, cloud access keys, database
URLs with embedded passwords, webhook signing secrets, deploy tokens.

### 3.2 Adversaries, in the order they actually matter

**A1 — The careless agent (primary).** An LLM coding agent running as your user,
with shell and filesystem access. It does not want your key. It will nonetheless
inline it into source, print it while debugging, or ingest it into a transcript
that leaves the machine. Its "attack" is disclosure through normal operation.

**A2 — Exfiltration through your own artifacts.** Git commits, CI logs, pasted
error output, screen shares, uploaded `.env` files. Often initiated by A1.

**A3 — Casual local snooping.** Another process on the machine — a random npm
postinstall script, a browser extension's helper, a curious background app —
reading well-known plaintext paths (`~/.codex/config.toml`, `~/.aws/credentials`,
`.env` files anywhere on disk).

**A4 — Offline access to disk.** Stolen laptop, unencrypted backup, a synced
folder that shouldn't have been synced.

**A5 — A determined local attacker with code execution as your uid.** Explicitly
**out of scope.** On a desktop OS, a process running as you can attach a debugger
to your other processes, read the memory of the daemon, drive the UI, or simply
call the IPC socket and impersonate the CLI. Keyward raises the cost and makes the
attempt loud (audit log, approval prompts), but it cannot win this fight, and any
design claiming otherwise is lying. Full mitigation requires OS-level sandboxing
that a userland tool cannot impose on itself.

### 3.3 What Keyward actually guarantees

1. No plaintext secret is ever written to disk by Keyward. At rest, everything is
   in the OS keychain (macOS Keychain / Windows DPAPI-backed Credential Manager),
   which handles A4.
2. No plaintext secret appears in any file Keyward writes or in any argv it
   constructs — configs get references, never values. This handles A2 and A3.
   **One exception, and it is explicit**: `kw render` (§8.1) exists to write a
   resolved file for tooling that cannot be wrapped. It is opt-in per invocation,
   refuses any path `.gitignore` does not cover, creates the file 0600, and says
   what it did. An exception the user typed is not the same as a guarantee with a
   hole in it — but a claim of "never" with a `render` command in the CLI would
   be, so it is stated here rather than three sections away.
3. Every disclosure of a plaintext value is recorded in an append-only log
   (called the *audit log* throughout this document; on disk it is `uses.jsonl`,
   and the UI calls it "used by" — see DESIGN.md §7 on keeping jargon out)
   with the requesting process's identity, and is subject to a per-secret policy
   that can require an interactive human approval. This is the only real defense
   against A1, and it is a *policy* defense, not a *technical* one.
4. In broker mode (§6), the plaintext value is disclosed to **no process at all**
   — not even the child. This is a genuine technical defense against A1, and it is
   Keyward's central idea.

### 3.4 What it does not guarantee

- It does not stop an agent that can run `kw exec -- env` from seeing a Tier-2
  secret. Tier-2 exists for cases where there is no alternative; the answer is to
  put such secrets behind an approval prompt, not to pretend they're protected.
- It does not defend against A5.
- It does not stop you from revealing a secret and then pasting it somewhere.

---

## 4. The disclosure tier model

This is the spine of the product. Every secret carries a **maximum tier**, and every
request is served at the lowest tier that can satisfy it.

| Tier | Internal name | Who sees plaintext | Use for |
|---|---|---|---|
| **T0** | Reference | Nobody. `keyward://…` is a name, not a value. | Anything an agent writes into a config file. |
| **T1** | Broker | Only `keywardd`. Traffic is proxied over loopback; the daemon swaps in the real credential on the way upstream. | HTTP APIs — LLM providers, REST services. **Default for API keys.** |
| **T2** | Injection | The spawned child process (via env). | Tools that must hold the credential themselves: `terraform`, `psql`, `docker login`, SDKs with no base-URL override. |
| **T3** | Reveal | The human, via clipboard or one screen. | Pasting into a web console. Always requires interactive approval; clipboard auto-clears. |

**"T0/T1/T2" is engineering vocabulary and must never appear in the UI — and
neither should the choice itself.**

An earlier draft of this document put a three-option segmented control on the
secret detail screen and let the user pick a tier. That was wrong, for the reason
most security UI is wrong: it asked a consumer to choose a security level, which
means the product's safety depends on the user understanding a model they have no
reason to learn. It also made the weaker options look like equally valid choices,
because a segmented control gives its options equal visual weight by construction.

**Keyward chooses, and reports what it chose.** At launch, for each secret:

> Can this credential be brokered? → broker it (T1).
> Otherwise → inject it (T2), and say so, once, in plain words.

The user sees a *status*, not a setting: **保护中** or **会共享**. There is no
control to get wrong, no default to regret, and no way to accidentally run in the
weaker mode when the stronger one was available. An override lives in an advanced
settings pane for the rare user who wants injection despite brokering being
possible; it is not on the main screen and is not part of the normal flow.

T0 is not a mode at all — references are simply what files contain, always.
T3 (reveal) is a right-click action with a confirmation, not a tier a secret sits
in. That leaves exactly two runtime behaviours, decided by the system.

A secret's **delivery mode** decides what it can be served at (§5). Requesting
above it fails, loudly and auditably. A brokered secret means **no process on the
machine can ever obtain the literal string** — and unlike the `max_tier` policy
field an earlier draft specified, this is not a setting that can be lowered: the
ceiling is computed from the credential's own shape, so there is no field for a
future feature to write to.

### Why T1 is worth building

The mature tools in this space (1Password's `op run`, Doppler, Infisical) all
implement T0 + T2: you write `op://vault/item/field` in a `.env`, and at launch the
real value is substituted into the child's environment. That fixes A2, A3, A4 —
genuinely useful, and if that's all you need, use `op run` today.

But against A1 it is thin: the agent is very often *the parent of, or a sibling to,*
the process that receives the injected value. `op run -- env` prints the secret.
An agent debugging a 401 will reach for exactly that.

T1 removes the value from the equation. The child is configured with
`base_url=http://127.0.0.1:PORT/v1` and a per-session loopback token; the daemon
attaches the real `Authorization` header on the way out. `env` shows the agent a
token that is worthless off this machine and expires with the session. This is the
one thing Keyward does that off-the-shelf tools do not.

It is also a real constraint: T1 only works for HTTP(S) credentials where the
client lets you point at a different base URL. Roughly all LLM providers qualify.
`psql` does not. Hence T2 continues to exist.

---

## 5. Data model

**One secret is one name and one value.** There is no profile, no field list and
no version, in the model or on the wire. That is the single largest change from
this document's first draft, and it came from §7: the moment `vault.list` had to
return one row per thing the user thinks of as "a secret", a `Profile` containing
`Field`s was a grouping the user maintained for the product's benefit rather than
their own — which is also what DESIGN.md §7's "don't group the list" forbids at
the other end of the stack.

```
Vault
├── schema: u32
└── secrets: BTreeMap<String, Secret>   keyed by name, so iteration order is the
                                        alphabetical order the list shows

Secret
├── name: String        the slug used in references. Immutable once created:
│                       changing it silently breaks every .env pointing at it
├── display: String     what the user sees. Free to change, any script
├── delivery: Delivery  Brokered { upstream, base_url_env? } | Handed
├── masked: Option      enough to recognise, never enough to use. Computed once,
│                       when the value is stored
├── revision: u32       bumped on rotation, so the new value lands under a fresh
│                       keychain account rather than overwriting in place
└── approval: Never | Ask

Use                     one line of the usage log
├── at: String          RFC 3339, stamped by the daemon, which owns the clock
├── secret / actor / project
├── caller: Option      the peer the daemon attested (§7), rendered
├── tier: Tier
└── allowed: bool
```

**`Delivery` replaces `Policy.max_tier`.** The old model let a policy field
declare a ceiling independently of what the credential actually was, which meant
two sources of truth for the one question the product turns on. Now the question
is asked once — *can Keyward stand in front of this?* — and the answer *is* the
ceiling: `Brokered` ⇒ T1, `Handed` ⇒ T2, computed by `choose_tier`, not stored and
not settable. A brokered secret cannot be handed to a process by any approval, any
setting or any caller, and `keyward-core`'s test suite asserts exactly that.

**`actor` and `caller` are both recorded and they are not the same thing.**
`actor` is whatever the caller said it was — a program name passed over the socket,
or a `User-Agent` header off a brokered request. It is the useful label and the
untrustworthy one. `caller` is what the kernel said. A log that cannot tell the two
apart cannot answer "did something impersonate `kw`", which is the only question
the log exists for.

Cut from the first draft and not replaced: `allowed_callers`, `ttl`,
`rate_limit`, `kind`, per-field `icon`/`color`. The first three are §11.5 entries;
the last two moved to the daemon, which derives an avatar letter, a tint and a
bundled brand-mark name per row so the two desktop apps cannot disagree.

### Storage split

- **Secret values** → OS keychain, one item per (secret, revision).
  - macOS: `kSecClassGenericPassword`, service `ai.keyward.vault`, account
    `<name>/v<n>`. Every item Keyward owns carries that service name, so a user can
    find and audit them all in Keychain Access under one heading.
  - Windows: Credential Manager generic credential, DPAPI-encrypted to the user.
    Not built — the `keyring` dependency is declared for `cfg(windows)`, but
    nothing else in the daemon compiles for Windows yet (§7's transport is
    `std::os::unix`).
  - **Only `keywardd` ever calls a keychain API**, via the Rust `keyring` crate,
    and inside `keywardd` only `store.rs` does. The GUIs do not — they are IPC
    clients like the CLI. This is worth stating explicitly because the obvious
    design (each native frontend uses its platform's native keychain API) would
    force the Swift and Rust sides to agree on item naming byte-for-byte, and
    would put plaintext in a second process for no benefit. One writer, one
    auditor, one place to get it right.
  - There is exactly **one function in the product that returns a plaintext
    value**: `SecretStore::reveal`. Auditing the claim in §3.3 means reading one
    file and finding its callers.
- **Metadata** → a plain JSON document at
  `~/Library/Application Support/Keyward/vault.json`, mode 0600, written
  atomically (temp file in the same directory, `fsync`, rename). Deliberately
  human-readable and containing zero secrets, so it can be version-controlled or
  synced without risk; a unit test asserts no field named `value` ever serialises.
  A parse failure **refuses to start the daemon** rather than falling back to an
  empty vault: presenting an empty list to someone whose secrets are still in the
  keychain invites the obvious next move — re-adding them — which would overwrite
  the file for real.
- **Usage log** → append-only JSONL at `uses.jsonl`, 0600, in the same directory.
  Named for what it is: it records disclosures and brokered requests, and it is
  read back by `uses.list` to fill the "who used it" screen. **Not yet rotated** —
  the 10 MB cap this section used to specify is unimplemented, and at one line per
  brokered request that is a real gap for a long-running session.

**Ordering rules that are load-bearing on write:** keychain before metadata on
add (so a failure leaves no metadata pointing at a value that was never stored);
metadata before deleting the old revision on rotate (so a crash mid-rotation
cannot lose the secret entirely); metadata before deleting the keychain item on
remove (so a leftover item is orphaned rather than dangerous).

---

## 6. Architecture

```
┌──────────────────────────────────────────────────────────────┐
│  keywardd  (the only holder of plaintext)                    │
│                                                              │
│   ┌──────────┐  ┌────────┐  ┌────────┐  ┌─────────────────┐  │
│   │ Keychain │  │ Policy │  │ Audit  │  │ Broker (T1)     │  │
│   │ adapter  │  │ engine │  │ log    │  │ loopback proxy  │  │
│   └──────────┘  └────────┘  └────────┘  └────────┬────────┘  │
│         ▲            ▲                           │           │
│         └────────────┴───── IPC server ──────────┘           │
└──────────────────────┬───────────────────────────┬───────────┘
             UDS / named pipe              127.0.0.1:PORT
                       │                           │
        ┌──────────────┼──────────────┐            │
        ▼              ▼              ▼            ▼
   Keyward.app     Keyward.exe       kw       child process
   (menu bar)        (tray)         (CLI)   (codex / claude / …)
```

### 6.1 Broker (T1) in detail

On `kw exec -- npm run dev`, for each `keyward://` reference in the `.env`:

1. CLI asks the daemon to open a **session** for the named secret (`broker.open`).
2. The daemon looks the secret up. If its `delivery` is not `Brokered` it refuses
   with `denied`, and the CLI falls back to `secret.hand` (§6.2). Otherwise it
   mints a random 32-byte token from `/dev/urandom`, rendered `kws_<hex>`, and
   registers a route: `token → (upstream, real credential, secret name, opened_at,
   request count, last User-Agent)`.
3. Daemon returns `{ session, base_url: "http://127.0.0.1:PORT", token,
   base_url_env }`.
4. CLI puts the **token** in the environment variable whose `.env` line held the
   reference, and — if the secret carries a `base_url_env` — adds that variable
   pointing at the loopback base URL. Then it spawns the child.
5. Every request the child makes hits the daemon, which validates the token,
   strips it, attaches the real `Authorization: Bearer …`, and forwards. Streaming
   (SSE) is re-chunked and flushed per 8 KiB read rather than buffered, so a
   response reaches the client as it arrives; no bodies are parsed.
6. On child exit the CLI calls `broker.close` and the token is dead.

**Why `base_url_env` is a field on the secret** (`Delivery::Brokered`) rather than
a lookup in a provider table. The first draft assumed a table in `keyward-core`
mapping provider → base-URL variable, so that "brokerable" meant "we recognise
this provider". That is the wrong shape twice over: it makes the set of brokerable
credentials a hardcoded list that ships with the binary, and it cannot express the
case that actually matters — a self-hosted or proxied endpoint whose credential is
an ordinary bearer token but whose variable name nobody can guess.

The consequence of getting it wrong is specific and invisible: without a
base-URL variable the child holds a session token and still calls the **real**
host with it, which fails as "invalid API key" inside someone else's error
message. So the field is optional and `kw exec` **says so out loud** when it is
missing —

```console
  stripe  forwarded at http://127.0.0.1:51823 — point the client at it
          (no base-URL variable is set for this secret)
```

— rather than printing "forwarded" and letting the user believe a secret is
protected when the program is about to bypass the broker entirely. A silent
downgrade is how a security tool loses its meaning; a silent *non*-downgrade that
simply doesn't work is worse.

Hardening details that matter:

- Bind to `127.0.0.1` explicitly, never `0.0.0.0`. ✅
- Reject any request whose `Host` header is not a loopback literal
  (`127.0.0.1`, `localhost`, `[::1]`) — DNS-rebinding defence. A hostile page can
  resolve a name it owns to 127.0.0.1, but it cannot make the browser send a
  loopback `Host`. ✅
- Reject requests carrying `Origin` or `Referer` headers **at all**; legitimate
  SDK traffic has neither, browser traffic does. ✅
- Drop hop-by-hop headers (`connection`, `keep-alive`, `transfer-encoding`,
  `upgrade`) in both directions rather than forwarding them into a connection that
  is being re-framed. ✅
- Per-session request counter and last-seen `User-Agent`, surfaced in the UI as a
  live card and a breathing dot, so unexpected traffic is visible while it
  happens. ✅ The session struct handed to the UI **carries no credential** — a
  type that could carry a secret into a view layer is a leak waiting for someone
  to log it.
- The broker is a dumb reverse proxy. It does **not** translate protocols. ✅
  (Beacon translates Responses↔Chat for Codex; that is a different product's job.
  If it is wanted here later it goes behind an explicit per-secret `transform`
  option.)
- Verify the peer of each *connection* is the session's process tree. ❌ Not
  implemented. Peer attestation exists on the **IPC** socket (§7) but not on the
  broker's TCP listener, where the peer of a loopback connection is any process on
  the machine that guessed a 64-hex token.

Three simplifications in the current implementation, each of which this document
previously described otherwise:

- **One port for the daemon, not one per session.** The listener is bound once at
  startup on an ephemeral port; sessions are distinguished by token, not by port.
  Per-session ports would give a second, weaker identifier for the same thing and
  would make the "which port is my broker on" question depend on which secret you
  meant.
- **`session` and `token` are the same string.** The response carries both keys
  for the shape the protocol promises, but a separate opaque session id would only
  be worth minting if something could hold one without holding the other, and
  nothing does.
- **No TTL and no path allowlist.** A route lives until `broker.close` or daemon
  exit. `kw exec` closes on child exit, so the practical lifetime is the child's —
  but a crashed CLI leaves a live route behind, and that is a real gap.

One upstream shape is not handled: the broker always attaches
`Authorization: Bearer`. It *accepts* `x-api-key` / `api-key` inbound as the
session token, so an SDK that authenticates that way can talk to it, but the
upstream request is always a bearer header. Anthropic's own API, which wants
`x-api-key`, therefore does not work through the broker today. §12.3 already flags
request-signing as out of scope for v1; per-upstream header *placement* is a
smaller gap and a closer one.

### 6.2 Injection (T2) in detail

Reached by `kw exec` when `broker.open` refuses, i.e. whenever a secret's
`delivery` is `Handed`:

1. CLI calls `secret.hand` with the names, an actor and a project. The daemon
   evaluates `evaluate(Injection, secret.tier(), secret.approval)` per name. A
   brokered secret is refused here — this branch is where the product's headline
   guarantee is *enforced* rather than merely described.
2. Daemon returns the values **over the IPC socket only** — never argv, never a
   temp file, never an env var of the CLI itself. It records a use only after
   every value resolved, so a partial failure cannot leave the log claiming a
   disclosure that never happened.
3. CLI builds the child's environment, **spawns** the child, and zeroizes its own
   copies immediately after.

**Step 3 says "spawns", and an earlier draft of this section said "`exec`s,
replacing itself so no extra process holds the values".** That was the better
answer for T2 in isolation and it is incompatible with §10.3: scrubbing the
child's stdout and stderr requires a parent holding the pipes. The choice is
between one extra process in the tree — which holds zeroized buffers and whose
environment does not contain the values — and giving up the only defence that
covers a library printing a secret it was legitimately given. The scrubber wins.

Residual exposure, stated honestly: the child's environment is readable by the same
uid on Linux (`/proc/<pid>/environ`), and by root on macOS/Windows. More
importantly the agent can simply run `kw exec -- env`. T2 secrets should carry
`approval: ask` if that matters to you — though see §11.4: the approval **prompt
is not wired up**, so a secret set to `ask` currently fails the request with
`approval_required` instead of asking anyone.

---

## 7. IPC protocol

**Transport.**
- macOS: Unix domain socket at
  `~/Library/Application Support/Keyward/keywardd.sock`, `chmod 0600` after bind.
  ✅ The containing directory is created but **not** forced to `0700` — a gap
  worth closing, since the socket's mode is the whole access control here.
- Linux: `$XDG_RUNTIME_DIR/keyward/keywardd.sock`. ❌ The daemon uses
  `std::os::unix::net` and would build, but the path is hardcoded to the macOS
  Application Support location.
- Windows: named pipe `\\.\pipe\keyward-<user-sid>`, created with an SDDL granting
  access only to the owning user SID and denying NETWORK. ❌

**Framing.** Newline-delimited JSON, one object per line, request/response
correlated by `id`. Chosen over gRPC/Cap'n Proto for one reason: the macOS client
is Swift and the Windows client is Rust, and hand-writing a JSON line codec twice
is cheaper than maintaining a shared IDL toolchain across both. It also means the
thing on the other end can be a two-hundred-line stub (§2.1), which is what made
the desktop app buildable before the daemon existed.

A 1 MiB frame cap is specified and **not enforced** — the daemon reads with
`BufRead::lines()`, so a peer that never sends a newline grows a buffer until the
process dies. It is a same-uid denial of service, which §3.2 A5 already concedes,
but it is cheap to fix and should be.

Connections are one thread each, and the Swift client keeps exactly one, issuing
requests serially and matching each to the next line read back. The `id` exists
for pipelining, and nothing in either app benefits from it — the main screen is a
single `vault.list` — so a request/response map would be state that can go wrong
for no gain.

**Peer attestation.** On accept, the daemon resolves the peer:
- macOS: `getsockopt(LOCAL_PEERPID)` → `proc_pidpath` → executable path. ✅
  Code-signing identity (`SecCodeCopyGuestWithAttributes` → identifier + team ID)
  is **not** resolved: §7 says unsigned and unknown binaries are labelled rather
  than blocked, so a path is enough to label one, and there is no safe Rust
  wrapper for the Security framework call worth the dependency yet.
- Windows: `GetNamedPipeClientProcessId` → `QueryFullProcessImageName` →
  Authenticode signer. ❌
- Linux: `SO_PEERCRED` → `/proc/<pid>/exe`. ❌

The two macOS syscalls are borrowed from `nix` and `libproc` rather than written
in-tree, because the workspace **forbids** `unsafe_code` and `forbid` is not
liftable by an inner `#[allow]`. That was the point of choosing `forbid`: the
choice becomes "take a thin, auditable dependency" rather than "quietly punch a
hole in the lint that keeps this daemon auditable".

Resolution is **best-effort by design**: a peer that cannot be resolved is served
and labelled `unidentified process (pid N)`, because refusing it would mean a
kernel that answered slowly could lock a user out of their own secrets.

The resolved identity is recorded in the usage log as `caller`, distinct from the
self-declared `actor` (§5). It is **not** matched against `allowed_callers` —
that rule is a §11.5 cut, and peer identity is recorded rather than enforced.
Unsigned or unknown binaries are not blocked — they are *labelled*, and the
approval prompt is meant to say so: "`/opt/homebrew/bin/kw` (unsigned) wants
stripe". Blocking by default would break every legitimate `cargo install`;
labelling puts the judgment where it belongs. `Caller::key()` returns `None` for
an unresolved peer specifically so a "remember this caller" answer is *not*
cached against a pid, which is reused within minutes and would hand a later,
unrelated process an approval a human gave to something else.

### 7.1 Methods

One secret is one name and one value, so every method keys on a name. There is no
profile, no field and no version in the wire format.

| Method | Params | Returns | Moves plaintext? |
|---|---|---|---|
| `daemon.status` | — | `version`, `stub`, `secrets`, `broker_port`, `broker_sessions` | no |
| `vault.list` | — | `secrets[]` — everything the main screen shows, including any live session | no |
| `uses.list` | `name?`, `limit=50` | `uses[]`, newest first | no |
| `vault.add` | `name`, `value`, `display?`, `delivery?` | `ok` | **in** |
| `vault.rotate` | `name`, `value` | `ok` | **in** |
| `vault.remove` | `name` | `ok` | no |
| `vault.set_display` | `name`, `display` | `ok` | no |
| `vault.set_approval` | `name`, `approval` ∈ {`ask`,`never`} | `ok` | no |
| `uses.record` | `name`, `actor?`, `project?` | `ok` | no |
| `secret.hand` | `names[]`, `purpose?`, `actor?`, `project?` | `values{}` | **out** |
| `broker.open` | `name`, `ttl_secs?` | `session`, `token`, `base_url`, `base_url_env`, `expires_at`, `ttl_secs` | no |
| `broker.sessions` | — | `sessions[]` — token, secret, opened/expires, request count, actor | no |
| `broker.close` | `session` | `closed` | no |
| `approval.pending` | — | `pending[]` — parked requests awaiting a human | no |
| `approval.resolve` | `id`, `decision` ∈ {`allow_once`,`allow_caller`,`deny`} | `resolved` | no |
| `scrub.values` | `grant` | — | *not implemented* |

Notes on individual methods, in the order they surprise an implementer:

- **`vault.list` returns presentation data the daemon computed**: `letter`,
  `tint` and `logo` alongside `name`/`display`/`ref`/`masked`/`status`/`last_use`.
  That looks like layering violated, and it is the deliberate kind: two frontends
  that each derive a monogram colour will eventually disagree about one, and the
  bug is invisible until someone puts the two apps side by side.
- **`vault.list` also carries `live`** when a broker session is open for that
  secret — `{session, requests, opened_at, actor}`. It carries no credential, on
  purpose: a struct that *could* is a leak waiting for a view layer to log it.
- **`vault.set_display` renames the label only.** The slug is immutable; changing
  it would silently break every `.env` pointing at the old reference, and a rename
  that breaks a project is not a rename the user asked for.
- **`uses.record` is a development affordance**, not part of the product: it
  writes a usage-log line for a secret so the "who used it" screen has real data
  to render before anything has actually used a key. It moves nothing and cannot
  fabricate a *disclosure* — the tier it writes is `broker`.
- **`secret.hand`, `approval.pending` and `approval.resolve` are dispatched
  before the vault lock is taken**, and must stay that way. `secret.hand` can park
  for up to sixty seconds waiting for a person, and the person's answer arrives on
  a *different* connection as `approval.resolve` — which would then block on the
  lock held by the request it is trying to release. That is a deadlock in the
  daemon's own approval prompt.
- **The approval loop fails closed.** A poisoned lock, a timeout, a shutdown:
  every path that is not an explicit allow resolves to a refusal, with distinct
  codes (`approval_denied` vs `approval_timeout`) because a timeout deserves a
  different sentence in the UI. A remembered `allow_caller` is keyed to
  `(attested identity, secret)` and never to a pid — "allow `kw`" and "allow `kw`
  to read the production database" are different sentences, and the prompt only
  ever asked the second one.
- **A refusal is recorded as a use** (`allowed: false`). It is the entry that
  tells a user something asked for their production database at 3am and did not
  get it, which is the question the "who used it" screen exists for.
- **`scrub.values` is unimplemented and returns `not_implemented`.** `kw exec`
  builds its scrubber from the values it already received via `secret.hand`, so a
  second method that returns plaintext to a process that already has it would be
  additional surface for no capability. It stays in the protocol for the case this
  document imagined it for — a process holding only a *session*, which has no
  values to scrub — and that case does not exist yet.

```jsonc
// Enumerate. Names, references, masks and last use — NEVER values.
// Safe for any caller, including the MCP server (§10.4).
→ {"id":1,"method":"vault.list"}
← {"id":1,"ok":true,"result":{"secrets":[
    {"name":"stripe","display":"Stripe","ref":"keyward://stripe",
     "masked":"sk_live_…4f2a","status":"protected",
     "letter":"S","tint":"#5B51E8","logo":"StripeLogo",
     "last_use":{"at":"2026-07-25T14:32:11Z","actor":"codex","project":"my-shop"},
     "live":{"session":"kws_91a…","requests":142,
             "opened_at":1769350331,"actor":"codex/1.2"}},
    {"name":"pg-prod","display":"生产数据库","ref":"keyward://pg-prod",
     "masked":null,"status":"shared","letter":"P","tint":"#2F6491",
     "logo":"PostgresLogo","last_use":null}]}}

// Hand plaintext to a child process. Policy + approval apply. Can park for up to
// 60s while a human answers, so it is dispatched without the vault lock.
→ {"id":2,"method":"secret.hand",
   "params":{"names":["pg-prod"],"purpose":"kw exec: npm run dev",
             "actor":"npm","project":"my-shop"}}
← {"id":2,"ok":true,"result":{"values":{"pg-prod":"postgres://…"}}}

// Open a broker session. The child gets a loopback URL and a session token.
// `base_url_env` is the variable that points the client at the broker; it comes
// from the secret, and its absence is reported rather than papered over (§6.1).
→ {"id":3,"method":"broker.open","params":{"name":"stripe"}}
← {"id":3,"ok":true,"result":{"session":"kws_91a…",
                              "base_url":"http://127.0.0.1:51823",
                              "token":"kws_91a…",
                              "base_url_env":"STRIPE_API_BASE",
                              "expires_at":1769353931,"ttl_secs":3600}}
→ {"id":4,"method":"broker.sessions"}
← {"id":4,"ok":true,"result":{"sessions":[
    {"session":"kws_91a…","secret":"stripe","opened_at":1769350331,
     "expires_at":1769353931,"requests":142,"actor":"codex/1.2"}]}}
→ {"id":5,"method":"broker.close","params":{"session":"kws_91a…"}}
← {"id":5,"ok":true,"result":{"closed":true}}

// The approval loop. The GUI polls for parked requests and answers them; the
// answer must arrive on a different connection from the one that is waiting.
→ {"id":6,"method":"approval.pending"}
← {"id":6,"ok":true,"result":{"pending":[
    {"id":"ap_3","secret":"pg-prod","caller":"/opt/homebrew/bin/kw (unsigned)",
     "actor":"kw","pid":40122,"purpose":"kw exec: npm run dev",
     "project":"my-shop","expires_in":47}]}}
→ {"id":7,"method":"approval.resolve",
   "params":{"id":"ap_3","decision":"allow_caller"}}

// Values Keyward must scrub from child output (§10.3). Reserved, not implemented
// — see the note above.
→ {"id":8,"method":"scrub.values","params":{"grant":"g_7f3…"}}

// Writes — the GUIs and `kw add`. Values travel over this socket and nowhere else.
→ {"method":"vault.add","params":{"name":"stripe","display":"Stripe",
                                  "value":"sk_live_…","delivery":{"kind":"brokered",
                                  "upstream":"https://api.stripe.com",
                                  "base_url_env":"STRIPE_API_BASE"}}}
→ {"method":"vault.rotate","params":{"name":"stripe","value":"sk_live_…"}}
→ {"method":"vault.remove","params":{"name":"stripe"}}
→ {"method":"vault.set_display","params":{"name":"stripe","display":"Stripe 生产"}}
→ {"method":"vault.set_approval","params":{"name":"pg-prod","approval":"ask"}}

// The "who used it" feed — half the product (DESIGN.md §1).
→ {"method":"uses.list","params":{"name":"stripe","limit":50}}

→ {"method":"daemon.status"}
```

`vault.add` takes `delivery` as an object; **omitting it, or omitting its
`upstream`, means `handed`**. Handing over is the safe default for an unknown
credential: brokering something that is not an HTTP bearer token fails at request
time, and a failure the user cannot diagnose is worse than a weaker mode they were
told about.

Errors: `{"id":n,"ok":false,"error":{"code":"denied","message":"…"}}`. The
`message` is written for a person to act on — `keyward-core`'s `Denial` renders
"this secret is forwarded by Keyward, so no program can receive its value — point
the program at Keyward instead of asking for the secret", not "Injection was
requested; Broker is available", which is accurate and tells the reader nothing
about what to do next.

| Code | Meaning |
|---|---|
| `bad_request` | Missing or malformed params, or a name that is not a valid reference. |
| `denied` | Policy refusal — above all, "this secret is brokered and cannot be handed over". Exit 77 in `kw`. |
| `approval_denied` | A human said no. Exit 77. |
| `approval_timeout` | Nobody answered within 60s. Exit 77. |
| `not_found` | No such secret, or an unknown method. |
| `name_taken` | `vault.add` refuses to overwrite; `vault.rotate` is how you replace. |
| `keychain_error` | The OS keychain refused. |
| `io_error` | `vault.json` unwritable, lock poisoned, broker would not open. |
| `not_implemented` | Reserved method (`scrub.values`), and everything the stub declines. |

The response envelope carries no `detail` object. An earlier draft specified
`detail: {available, requested}` on a denial, so a caller could branch on the two
tiers; nothing needs to. A CLI that branches on "you could have brokered this"
would be building a retry the daemon already performed, and `kw exec` gets the
same information by *trying* `broker.open` first.

Two notes for implementers:

- **`vault.list` is the only method the GUI needs for its main screen**, and it
  returns everything that screen shows. Keep it that way; a list view that fans out
  into one request per row is how a menu-bar app starts feeling slow.
- **`secret.hand` takes names, not references.** Parsing `keyward://` is the CLI's
  job, done once when it reads the `.env`; the daemon should not need a URL parser
  on its request path.

---

## 8. Reference format

```
keyward://<name>
kw://<name>                                          # accepted short form
```

Grammar:

```
ref    ::= scheme "://" name
scheme ::= "keyward" | "kw"
name   ::= [a-z0-9]([a-z0-9-]{0,62}[a-z0-9])?
```

**A reference is one segment.** An earlier draft had
`keyward://<profile-slug>/<field-key>[@<version>]`, which modelled the
profile-and-fields data structure §5 no longer has. The parser now gives `/` and
`@` their **own error messages** rather than a generic "invalid name", because
both are things a person will reasonably try — the two-segment form was this
document's own syntax, and it is what every comparable tool uses:

```
keyward://stripe/api_key  →  a reference is a single name:
                             `keyward://stripe`, not `keyward://stripe/key`
keyward://stripe@4        →  version pinning is not supported yet;
                             use `keyward://name`
```

Reserving `@` by rejecting it with a specific message is how §11.5's version-pinning
cut stays reversible: the syntax cannot be taken by anything else, and the day it
returns nobody's `.env` changes meaning underneath them.

Two parser decisions worth stating because they look like bugs:

- **No whitespace trimming.** `" keyward://stripe"` fails. A reference with
  surrounding whitespace came from something that mangled it, and guessing is how
  a parser starts accepting input its callers did not mean.
- **`Reference::slugify` returns `None` rather than inventing.** A display name of
  only CJK or emoji has no ASCII slug, and transliterating or hashing would hand
  the user a reference they cannot recognise in their own `.env`; the UI asks them
  to type one instead. The Swift side mirrors this function *including the
  refusal*, since the add sheet previews the reference live.

Design constraints this format satisfies:

- **Greppable by a dumb regex.** `\bkw(?:eyward)?://[a-z0-9-]+\b` — so a
  pre-commit hook can assert "no bare `sk-` strings, refs are fine", and an agent
  hook can whitelist it. `find_all` implements this over arbitrary text with byte
  spans, so `kw render` can splice a resolved value into the exact range without
  searching twice; it refuses near-misses (`xkw://a`, `my-keyward://a`) by
  requiring a non-identifier character before the scheme.
- **Obviously not a secret.** A human reading a diff sees a name, not entropy. This
  matters more than it sounds: the failure mode being prevented is a human
  approving a PR that contains a live key.
- **Safe to commit.** A `.env.example` full of refs is a complete, working config
  for anyone who has the same secret names locally. This is a real ergonomic win.
- **Deliberately not `${VAR}`.** Shell-looking syntax gets expanded by something
  unexpected eventually.

Resolution contexts: `.env` files (`kw exec`) and any file passed to `kw render`.
The `KEYWARD_REF` convention this section used to name — a single environment
variable holding one reference, for tools that read exactly one credential — was
never built and has no caller; it is not in the cut list because nothing was ever
decided about it, only never needed.

### 8.1 `.env` files

The most requested integration, and the one with a non-obvious failure mode worth
specifying rather than discovering.

**The file always holds references, never values, and is safe to commit:**

```dotenv
# .env — committable
ANTHROPIC_API_KEY=keyward://anthropic
DATABASE_URL=keyward://pg-local
STRIPE_SECRET_KEY=keyward://stripe-test
PUBLIC_API_BASE=https://api.example.com          # untouched, not a ref
```

Quoting is tolerated (`KEY="keyward://stripe"`), comment lines are skipped, and a
line whose value is not a well-formed reference is left entirely alone.

**The catch.** A static file cannot carry a T1 broker session — session tokens are
minted per launch and die with the process, so there is nothing stable to write
down. A naive implementation would resolve every ref to plaintext and inject it,
silently dropping every `.env`-based project from T1 to T2. That is the difference
between the product's headline guarantee and a `op run` clone, so `kw exec` does
something more specific:

**Resolution rules.** `kw exec` collects every reference in the file, then, per
secret:

1. **Try to broker, always.** It calls `broker.open` for every name before it
   asks for a single plaintext value. Handing a value over is the fallback, never
   the default — otherwise a secret that *could* have been protected quietly is
   not. If the secret's `delivery` is `Brokered`, the credential slot in the
   environment gets the **session token**, and the secret's `base_url_env`
   variable is added pointing at the loopback broker even though it was not in the
   file. The real key never leaves the daemon. **T1.**
2. **`denied` means fall back.** A `denied` from `broker.open` is the daemon
   saying "this is not an HTTP credential"; those names are collected and asked
   for in a single `secret.hand`, and the plaintext is put in the child's
   environment. **T2**, and `kw exec` says so per variable, every run. Silent
   downgrades are how a security tool loses its meaning.
3. **Anything else fails the command.** A refusal that is not `denied` — approval
   denied, approval timed out, keychain error — exits with the daemon's code (77
   for the policy ones, §9). No partial environment.

```console
$ kw exec -- npm run dev
  ANTHROPIC_API_KEY  forwarded — `anthropic` never leaves Keyward
  DATABASE_URL  handed over — the value is in this process's environment
  STRIPE_SECRET_KEY  forwarded — `stripe-test` never leaves Keyward
```

The notice prints on **every** run, not once per project as this section used to
say. A once-per-project notice needs somewhere to remember "this project has been
told", and the only honest places are a state file (which is a second thing to
keep 0600 and to explain) or the project directory (which is a file Keyward wrote
into a repo without being asked). Three lines of stderr on every launch is the
cheaper answer, and it is the line a user should be able to check.

`kw exec` closes every session it opened when the child exits — a token that
outlives the process it was minted for is a credential lying around with nothing
watching it. The daemon's TTL (§6.1) is the backstop for the case where `kw`
itself dies first.

**Precedence.** `kw exec` sets real variables in the child's environment *before*
the app's own dotenv library reads the file. The common libraries — Node `dotenv`,
`python-dotenv` — do not override variables already present in the environment, so
the injected value wins and the library harmlessly re-reads the ref string into
nothing. This layering is load-bearing and must be verified per framework, not
assumed: bundlers with their own precedence rules (Vite, Next.js) need checking, and
where the layering doesn't hold, `kw render` (below) is the fallback.

**When the process isn't launched by `kw`.** An IDE-started dev server, a Docker
Compose `env_file:`, a `launchd` job — nothing resolves the refs, and the app
receives the literal string `keyward://anthropic`. This fails, which is
correct, but it fails inside someone else's error message ("invalid API key"). Two
mitigations, both required:

- `kw render .env.tmpl -o .env.local` writes a resolved file for tooling that
  can't be wrapped. ✅ Built. It refuses to write anywhere `.gitignore` does not
  cover — asking `git check-ignore`, and treating "no repository" or "no git" as a
  refusal, because **unknown is not permission** — and it sets mode 0600 *at
  creation* rather than after, so the file is never briefly world-readable. The
  gitignore check runs **before the daemon is contacted**, so a refusal costs no
  approval prompt and never asks for plaintext it would then have nowhere safe to
  put. On success it prints what it did and points the user back at `kw exec`.
  **Documented as the escape hatch of last resort, not as a supported workflow.**

  One promise from the earlier draft is *not* kept: the rendered file is **not**
  registered for deletion on daemon shutdown. That needed the daemon to track
  paths the CLI wrote, which is a list of plaintext file locations living in the
  one process that is supposed to be auditable — and a cleanup that runs only on a
  graceful shutdown is a guarantee that fails exactly when it matters. Telling the
  user to delete it is weaker and honest.
- **Project tokens** for the IDE case: a long-lived, machine-local, project-scoped
  broker token that *can* be written into a file, served from a stable port rather
  than an ephemeral one. Strictly weaker than a session token (it outlives the
  process and doesn't die on exit) but strictly stronger than plaintext (useless on
  any other machine, revocable from the UI, and it appears in the audit log). Not in
  v1 — see §12.9.

---

## 9. CLI surface

```bash
# --- discovery: safe for an agent to run, returns no plaintext, ever ---
kw list                                # table: name, masked, status, last use
kw list --json                         # machine-readable, for agent consumption
kw ref anthropic                       # prints: keyward://anthropic
kw status                              # is the daemon up, how many secrets, socket

# --- running things: one command, which picks the strongest tier per secret ---
kw exec -- npm run dev                 # resolves keyward:// refs found in ./.env
kw exec -f .env.local -- pytest
kw render config.toml.tmpl -o config.toml   # last resort; writes plaintext to disk

# --- management (these are the ones a human uses) ---
kw add anthropic                       # prompts on a TTY; never takes a value in argv
kw rotate anthropic
kw rm anthropic

# --- the agent ecosystem ---
kw scan [--staged] [path]              # find literal secrets; exit 1 on a hit
kw mcp                                 # MCP server on stdio: names and refs only
```

**There is one run command, not three.** The earlier surface had `kw broker -p X
-- claude` for T1, `kw run -p X -- terraform` for T2 and `kw exec` for `.env`
files, which put the tier choice back in the user's hands through the back door —
exactly what §4 spends a page arguing against. `kw exec` reads the file, asks the
daemon to broker everything, and falls back per secret. A user who types the wrong
command can no longer get a weaker mode than the one available, because there is
no wrong command to type.

Not built, and named here so their absence is a decision rather than an oversight:
`kw policy set` (`vault.set_approval` exists on the wire, and the GUI is the
intended surface for a two-value switch), `kw audit --follow` (the usage log is
readable at `uses.jsonl`, and the live view is the app's job — see UC-5),
`kw shell-init` (cut for good — §1.5), and `kw broker --print-env` (a printed session token is a
credential in a scrollback with nothing to close it).

Hard rules:

- **No subcommand ever accepts a secret value as an argument.** Values come from a
  TTY prompt or stdin. Argv is world-readable on Linux and lands in shell history
  everywhere.
- `kw list`, `kw ref`, `kw status` and `kw scan` are the *only* commands an agent
  should routinely call, and none can return plaintext regardless of policy.
  Documented as such so users can allowlist them in their agent's permission
  config without thinking.
- Exit code 77 (`EX_NOPERM`) specifically for `denied` / `approval_denied` /
  `approval_timeout`, so scripts can distinguish "you're not allowed" from "it
  broke". 127 for "could not run the child", 2 for a usage error, 1 for everything
  else.
- **The CLI reads a value from a TTY prompt or from stdin, and from nowhere
  else** — no `--value`, no `--from-file`, no environment variable. It zeroizes
  its buffer as soon as the daemon has acknowledged the write.
- **The daemon is never started by the CLI.** `kw` reports "Keyward isn't running.
  Open the Keyward app, then try again." A CLI that silently launches a background
  service holding secrets is exactly the behaviour a user should be suspicious of
  (§12.1), and this is the resolution of that open question in the current code.

---

## 10. The three surfaces

A secret can reach an agent's context window by three routes. Blocking one and
ignoring the others is not a partial solution — it is no solution, because the
value only has to leak once. All three are v1 scope.

### 10.1 Files the agent reads

`.env`, `config.toml`, `settings.json`. Covered by §8.1: files hold references,
`kw exec` resolves at launch. This is the surface everyone thinks of first.

### 10.2 MCP server configuration

Less obvious and arguably worse. An MCP server is configured in `mcp.json` or
`~/.claude.json` with an `env` block, and people put live credentials there:

```jsonc
{ "mcpServers": { "stripe": {
    "command": "npx", "args": ["-y", "@stripe/mcp"],
    "env": { "STRIPE_SECRET_KEY": "sk_live_51H…" } } } }   // ← plaintext, agent-readable
```

Two problems at once: the file is plaintext and sits in a directory the agent
reads freely, *and* the MCP server is a subprocess whose output flows straight
into the conversation.

Keyward rewrites the launch instead:

```jsonc
{ "mcpServers": { "stripe": {
    "command": "kw", "args": ["exec", "--", "npx", "-y", "@stripe/mcp"],
    "env": { "STRIPE_SECRET_KEY": "keyward://stripe/secret" } } } }
```

The config now holds a reference; `kw exec` resolves it as the server starts, and
scrubs the server's output (§10.3) on the way back. **The rewrite itself is not
built** — nothing in Keyward edits an MCP config today. What exists is
`kw scan`, which will *find* the literal key in `mcp.json` and name the file, and
`check_project` in the MCP server (§10.4), which does the same thing when an agent
asks. The user still edits the JSON. Doing it for them is §10.5 work.

### 10.3 Output the agent reads

The surface that no amount of reference-rewriting closes. A third-party library
logs its config at startup. An HTTP client's verbose mode prints request headers.
A stack trace includes the connection string. A test failure dumps the whole
environment. All of it lands in the agent's context, with the real value in it,
*after* every file on disk was already clean.

So `kw exec` scrubs the child's stdout and stderr: any occurrence of a value
Keyward knows is replaced, in the stream, with its reference.

```console
$ kw exec -- npm run dev
  [stripe] initialised with key keyward://stripe/secret
                                ^^^ the library printed the real key; the agent sees this
```

Implementation constraints worth stating, because a scrubber that is wrong about
its own limits is dangerous:

- Match on a rolling buffer, not per-write, or a value split across two `write()`
  calls slips through. Buffer at least `max_secret_len - 1` bytes across chunk
  boundaries.
- Scrub **before** the first byte reaches the terminal, never retroactively.
- Also scrub common encodings of each value: base64, URL-encoded, and JSON-escaped.
  Beyond that, stop — a library that hashes or chunks a secret defeats this, and
  pretending otherwise would be worse than the honest limit.
- Never scrub the *reference* itself, and never scrub short values that would
  produce false positives across unrelated output.

**This is best-effort and must be labelled as such**, in the docs and in the UI.
It is a net that catches the common accident, not a guarantee. The guarantee is
brokering (§6.1), where there is no value in the child to print in the first
place. Scrubbing is what protects the credentials that cannot be brokered.

### 10.4 The agent-facing API: references only

An earlier draft cut the MCP server, reasoning that the agent has no legitimate
need to query the vault. That was wrong, and the counter-example is the way people
actually talk to coding agents:

> "去 Keyward 拿 Stripe 密钥放到 .env 里"
> *("Get the Stripe key from Keyward and put it in .env")*

The agent **is** the thing wiring the project up. It needs to know which secrets
exist and what to write. Without an interface it either guesses the reference
syntax or asks the user to paste a key — the exact outcome this product exists to
prevent. So `kw mcp` ships, with three tools:

| Tool | Returns |
|---|---|
| `list_secrets()` | names + masked values + last use. **Never a value.** |
| `get_reference(name)` | `keyward://stripe` |
| `check_project(path)` | which files still hold literal secrets |

**No tool returns plaintext, and none can be made to.** Not "returns plaintext if
policy allows" — there is no code path. This is what makes the interface safe to
hand to the threat actor: enumeration is harmless when the enumerated thing is a
list of names.

So the sentence above resolves like this:

1. Agent calls `get_reference("stripe")` → `keyward://stripe`
2. Agent writes `STRIPE_SECRET_KEY=keyward://stripe` into `.env`
3. Agent replies: "done — `.env` points at Keyward; run it with `kw exec`"

The user got exactly the interaction they asked for. The agent never saw the key.

**If the user explicitly asks for the literal value**, the agent has no tool that
can supply one and says so. That refusal is the product working, not failing —
and it is worth making the tool descriptions say why, so the agent explains rather
than hunting for a workaround.

**Tool descriptions carry the operating instructions**, because the model reads
them on every call and they cannot be deleted the way a `CLAUDE.md` section can:

```
get_reference — Returns the keyward:// reference for a stored secret. Write this
string into .env / config files instead of a real key; it is safe to commit.
Keyward resolves it at launch, so the project must be run with
`kw exec -- <command>` — tell the user that when you write one. Real secret
values are never available through this server. If the user asks for the literal
key, say that Keyward does not expose values and that the reference is what their
config should contain; that refusal is the tool working correctly, so do not look
for a way around it.
```

The last sentence is the one that had to be written twice. "Values are never
available" states a fact; an agent that has just been told no by a tool will look
for another tool. Naming the refusal *as correct behaviour* is what stops the
search — and it is why the same wording appears in the server's `instructions`
field, which is delivered once at initialise and read as standing policy.

This is belt-and-braces with §1.5's `CLAUDE.md` block, deliberately: the MCP
description covers agents that have the server connected, the `CLAUDE.md` block
covers the ones that don't, and each states the `kw exec` requirement.

✅ Built as `kw mcp` — a JSON-RPC server on stdio, hand-rolled rather than via an
SDK, with exactly the three tools above. `check_project` shares its detector with
`kw scan`.

**A pre-commit hook** (`kw scan --staged`) that fails on strings matching known
key prefixes (`sk-`, `sk_live_`, `AKIA`, `ghp_`, `xoxb-`, …) plus a deliberately
narrow high-entropy rule, while allowing `keyward://` refs. ✅ Built. Exits 1 on a
hit, so it works as a hook without a wrapper.

Two properties decide whether it is usable at all, and both are design
constraints rather than tuning:

- **A `keyward://` reference is never a finding.** A hook that fires on the fix it
  is recommending gets uninstalled within a day.
- **False positives cost more than misses here.** A scanner that flags every git
  SHA and every long identifier trains its user to pass `--no-verify`, and then it
  catches nothing at all. So known prefixes carry most of the detection, and the
  entropy rule only fires on a token in assignment position.

Its output is masked **harder than the rest of the product** — the ordinary
`mask()` keeps seven leading and four trailing characters, which is right for a
list a user is scanning for "which key is this", and wrong for a report that names
a live, unrotated secret and is printed by a hook into a terminal scrollback.

---

## 10.5 The no-terminal path

Vibe coders are in scope, which has one hard consequence: **every protection must
be reachable without typing a command.** A GUI that only views what the CLI
configures serves the users who least need help. The two flows below are the whole
product for someone who never opens a terminal, and both must exist in M2.

> **Neither is built.** The Mac app today can add, rename, replace and delete a
> secret, show who used it, stop a live session, and generate the agent
> instructions (§1.5) — but it edits no project file and launches no process. That
> makes this section the largest single gap between this document and the app, and
> the reason M2 is not finished. It is recorded here rather than quietly deferred
> because the argument above is still the right one: if a user has to type
> `kw exec` for the protection to apply, the audience this section is about does
> not get protected.

**Import.** Drag a project folder onto Keyward. It finds `.env`, `.env.local`,
`mcp.json`, `.claude.json`; shows what it found with values masked; and on one
click imports each value into the keychain, rewrites the file to hold references,
and adds `.env` to `.gitignore` if it isn't there.

This is the single most valuable screen in the product. It converts an existing
unsafe project in one drag, requires no understanding of anything, and produces a
visible before/after the user can check. It is also what makes the first run
useful — the alternative onboarding, "add your secrets one at a time", asks for
work before delivering anything.

Care required, since it edits the user's files: show a diff before writing, never
touch a file not covered by `.gitignore` without saying so, and keep a one-click
undo for the whole import. A tool that silently rewrites source files is a tool
people uninstall.

**Run.** A "Run with Keyward" action in the GUI, per project, that runs the
project's dev command through `kw exec` — reading the script from `package.json`,
or whatever the user last used. Output appears in a panel with scrubbing (§10.3)
already applied.

The CLI keeps existing and stays first-class; it is what the GUI drives, and what
users who prefer terminals will use directly. It is simply not the only door.

---

## 11. Milestones

Ordered for a **consumer product**, which is not the order a developer tool would
use. The temptation is to ship the CLI first because it's the part that works
without design work — resist it. A CLI-first launch defines Keyward publicly as a
developer utility, and that framing is very hard to walk back later when you want
to charge a non-CLI price to a non-CLI audience. The CLI is an *implementation
detail of the product*, not the product.

**M0 — Core, unreleased.** Rust workspace: `keyward-core`, `keywardd`, `kw`.
Keychain adapters (macOS + Windows), JSON metadata, IPC with peer attestation,
audit log, T0/T2. Driven only by the CLI, because that's the cheapest harness. Not
published, not announced. *This is the part that has to be correct; everything
after is presentation.*

**M1 — Broker (T1).** Loopback reverse proxy, session tokens, SSE passthrough,
provider env-var table. Also unreleased. The differentiator has to exist before
there's anything worth selling.

**M2 — macOS app → first public release.** SwiftUI menu bar, `SMAppService` daemon
lifecycle, add/edit flow, the automatic status from §4 (**not** the three-option
control an earlier version of this list still called for — §4 cut it, and a
milestone that asks for a picker the design forbids is how a cut feature comes
back), approval prompts, usage viewer, Sparkle auto-update. Signed with a
Developer ID cert and notarized. `kw` bundled and offered on `PATH`. The public story on day one is "a Mac app that keeps
API keys out of your AI's hands", with a CLI mentioned in paragraph four.

The Swift app touches no keychain API and links no Rust code (§5, §2.1) — it
speaks the §7 protocol and nothing else. The integration surface to test is
therefore the protocol itself: a golden-file suite of request/response lines that
both the Swift client and the Rust daemon are checked against.

**M3 — Monetization.** Offline Ed25519 licence files (§13.3), a 14-day trial, a
purchase page. Deliberately *after* the first release: sell once people are using
it, and price against observed behavior rather than a guess.

**M4 — Windows app.** Tauri 2 tray, same protocol, Rust half shared with the
daemon. Authenticode signing (an EV cert or a few months of SmartScreen
reputation-building — budget for this, it's the tax on Windows distribution).

**M5 — Agent ecosystem.** MCP server, `kw scan` pre-commit hook, ready-made
allowlist snippets for Claude Code / Codex / Cursor.

Linux: the daemon and CLI build there essentially for free (Secret Service via
`keyring`). No GUI, no installer, no support commitment — a documented
`cargo build` for people who want it. It is not a target market.

---

## 11.4 Current state

Read off the code, not off the milestone list above. **A snapshot** — the tree
moves faster than this section does, so treat a ✅ as "there is an implementation
to read" and a ❌ as "there is nothing to read", and check the file named in
brackets when it matters.

### M0 — Core

| Piece | State |
|---|---|
| Rust workspace, four crates | ✅ `keyward-core`, `keywardd`, `kw`, `keywardd-stub` |
| Vault model, one-name-one-value | ✅ [`core/model.rs`] |
| Reference grammar + text scanner | ✅ [`core/reference.rs`], 12 tests |
| Policy engine (`choose_tier`, `evaluate`) | ✅ [`core/policy.rs`], 9 tests, including "a brokered secret can never be handed over, under any approval" |
| Keychain adapter (macOS) | ✅ [`keywardd/store.rs`], service `ai.keyward.vault` |
| Keychain adapter (Windows) | ❌ dependency declared, nothing else compiles for Windows |
| `vault.json` metadata, atomic write, 0600 | ✅ |
| Usage log (`uses.jsonl`) | ✅ append-only, 0600. **No rotation** — the 10 MB cap in §5 is unimplemented |
| IPC server, NDJSON over a Unix socket | ✅ macOS path only |
| Peer attestation | ✅ macOS (pid + executable path, via `nix`/`libproc`). ❌ Linux, Windows. ❌ code-signing identity anywhere |
| T0 (references) | ✅ |
| T2 (`secret.hand` + injection) | ✅ |
| T3 (reveal) | ❌ no wire method, no UI. `SecretStore::reveal` is the daemon's internal read, not a disclosure path to a human |
| Approval prompts | ✅ daemon side: park, poll, resolve, 60s timeout, fail-closed, remembered per (caller, secret) [`keywardd/approval.rs`], 11 tests. ❌ **no UI answers them** — see below |

### M1 — Broker (T1)

| Piece | State |
|---|---|
| Loopback reverse proxy | ✅ [`keywardd/broker.rs`] |
| Session tokens (32 bytes from `/dev/urandom`) | ✅ |
| Streaming passthrough | ✅ re-chunked and flushed per 8 KiB read |
| `Host` check, `Origin`/`Referer` rejection, hop-by-hop stripping | ✅ |
| TTL + reaper | ✅ default 3600s, expiry checked per request and swept |
| Request counting, live sessions surfaced to the UI | ✅ |
| Peer verification on *broker* connections | ❌ any local process with the token is served |
| Non-bearer upstream auth | ❌ always `Authorization: Bearer` outbound |
| Provider env-var table | cut — replaced by `Delivery::Brokered { base_url_env }` (§6.1) |

**The gap that matters most in M1 is not in the broker.** Nothing in the product
can currently *create* a brokered secret. `vault.add` takes a `delivery` object,
but `kw add` sends none and the Mac app's add sheet passes `upstream: nil` — so
every secret added through either frontend is `Handed`, and the entire T1 path is
reachable only by hand-editing `vault.json`. The differentiator is built and
unreachable, which is worse than either.

### M2 — macOS app

| Piece | State |
|---|---|
| SwiftUI app, `NavigationSplitView`, list + detail | ✅ |
| Menu-bar status item + popover | ✅ `NSStatusItem`, not `MenuBarExtra` (DESIGN.md §4) |
| Add / rename / replace / delete | ✅ |
| Usage viewer: table, 14-day activity strip, live session card | ✅ |
| Agent-instructions sheet (§1.5) | ✅ generated from the live vault |
| Settings: appearance, language, launch-at-login | ✅ |
| Localisation (en, zh-Hans) with an in-app switch | ✅ |
| Off-screen snapshot renderer for design review | ✅ `Keyward --snapshot <dir>` |
| App icon | ✅ 16–1024 |
| Approval prompt UI | ❌ the daemon parks the request and nothing ever calls `approval.resolve`, so a secret set to `ask` times out after 60s |
| Creating a brokered secret | ❌ see M1 |
| Drag-a-project import (§10.5) | ❌ |
| "Run with Keyward" (§10.5) | ❌ |
| `SMAppService` **daemon** lifecycle | ❌ the login item registers *the app*; nothing starts, stops or supervises `keywardd` |
| `kw` bundled and offered on `PATH` | ❌ no copy phase in `project.yml` |
| Sparkle auto-update | ❌ |
| Developer ID signing, notarisation, hardened runtime | ❌ `CODE_SIGN_IDENTITY: "-"`; hardened runtime is on |
| Golden-file protocol suite checking both sides | ❌ the Swift client has no tests |

### M3 — Monetization

❌ Nothing. No licence verification, no trial, no purchase page. `Cargo.toml`
points `license-file` at a `LICENSE.md` **that does not exist in the tree** —
the FSL decision in §13.2 is now committed to `LICENSE.md`.

### M4 — Windows

❌ Nothing. No Tauri project, no named-pipe transport; `keywardd` uses
`std::os::unix::net` unconditionally.

### M5 — Agent ecosystem

| Piece | State |
|---|---|
| MCP server | ✅ `kw mcp` — JSON-RPC on stdio, three tools, none able to return a value [`kw/mcp.rs`], 7 tests |
| `kw scan` / `kw scan --staged` | ✅ prefix table + narrow entropy rule, exits 1 on a hit [`kw/scan.rs`], 10 tests |
| `kw render` | ✅ refuses any path `.gitignore` does not cover, 0600 at creation [`kw/render.rs`] |
| Output scrubbing (§10.3) | ✅ rolling buffer across chunk boundaries, plus JSON-escaped / percent-encoded / base64 forms; drops needles under 8 bytes [`kw/scrub.rs`], 5 tests. Brokered secrets are scrubbed from *response bodies* by the broker instead, since `kw` never holds their value [`keywardd/broker.rs`], 4 tests |
| Ready-made allowlist snippets for Claude Code / Codex / Cursor | ❌ |
| `kw shell-init` | ❌ cut for good — the user it served does not run their own projects (§1.5) |

M5 ran ahead of M2 — which is worth noticing rather than celebrating. The
milestone order in §11 exists because a CLI-first launch defines the product
publicly as a developer utility, and the parts of M5 now built are exactly the
CLI-shaped ones. The remaining M2 work (approval UI, brokered-secret creation,
import, daemon lifecycle, signing) is what stands between this and something a
non-terminal user can install.

### Verification

`cargo test --workspace` covers 74 tests across the four crates, and none of them
need a keychain, a socket or a GUI — that is the point of `keyward-core` doing no
I/O. There is no Swift test target and no cross-language golden-file suite, which
means the §7 protocol is currently checked by running the two halves together and
looking.

---

## 11.5 Cut list

Recorded because a scope that isn't written down grows back. Each of these was in
an earlier draft and is deliberately **not** in v1:

| Cut | Why |
|---|---|
| Provider switching / LLM-provider profiles | That is Beacon's product, not this one (§1). Keyward stores an LLM key the same way it stores a Stripe key: as a project secret. |
| Configuring the agent's own credentials (`apiKeyHelper`, `ANTHROPIC_BASE_URL` in `~/.claude/settings.json`) | Same reason. The agent is the threat, not a client to be provisioned. |
| ~~MCP server~~ — **reinstated and built**, see §10.4 | Cut in an earlier draft, restored once it was clear that "get the Stripe key from Keyward and put it in .env" is how people actually instruct an agent. Ships as `kw mcp`, returning references only. |
| User-facing tier choice | Replaced by automatic selection + status (§4). Enforced by construction now: `Tier` is computed from `Delivery`, is not stored, and there is no wire method that sets it. |
| ~~Profiles and fields~~ — **cut later than the rest**, see §5, §8 | Not in the original cut list because it was the data model, not a feature. One secret is one name and one value; `keyward://profile/field` and `@version` now parse to their own error messages. |
| `allowed_callers` / code-signing allowlists | Real value, but it is policy depth for an audience that hasn't asked yet. Peer identity is now genuinely *recorded* — the usage log's `caller` field carries what the kernel said, distinct from the self-declared `actor` — just not *matched* against a rule. |
| Approval modes (`first_use` / `every_use` / `after_idle`) | Collapsed to one switch per secret: ask before sharing, or don't. The *answer* has three shapes (`allow_once` / `allow_caller` / `deny`), which is where `first_use` went: it is a property of the reply, not a mode to configure in advance. |
| Version pinning (`@4`) | Rotation stays; pinning is an incident-forensics feature. The grammar reserves the syntax **by rejecting it with a specific message**, so it can return without a breaking change and cannot be claimed by anything else meanwhile. |
| Provider env-var table | Cut on contact with the code (§6.1). A table of provider → base-URL variable makes brokerability a hardcoded list; `Delivery::Brokered { base_url_env }` puts the answer on the secret, where a self-hosted endpoint can have one too. |
| Per-session broker ports | Never built. One daemon port, sessions keyed by token (§6.1) — a second identifier for the same thing. |
| `KEYWARD_REF` single-variable convention | Named in §8, never built, no caller. |
| Rendered-file auto-deletion on daemon shutdown | Cut (§8.1): it would make the daemon keep a list of plaintext file locations, and a cleanup that only runs on a graceful exit fails exactly when it matters. |
| Project tokens | Deferred (§12.9). Reconsider only if the `kw exec` wrapper proves insufficient in real use. |
| Team sync / accounts | Already a non-goal (§2). |

The through-line: keep everything that serves "my project's secrets are not in a
file my agent can read," cut everything that serves "manage all my credentials."

---

## 12. Open questions

1. **Daemon autostart on first CLI use?** — **settled in the code: no.** `kw`
   checks for the socket and says "Keyward isn't running. Open the Keyward app,
   then try again." A CLI silently launching a background service that holds
   secrets is exactly the behavior one should be suspicious of. The other half of
   the answer is not built: the GUI's login item registers *the app*, and nothing
   yet starts `keywardd` at all (§11.4).
2. **Locking.** Does the vault lock on screen-lock / after idle, requiring Touch ID
   or Windows Hello to unlock? This is the single biggest UX-vs-security dial. A
   reasonable default: T0/T1 work while locked (sessions already open keep
   working), T2/T3 require unlock.
3. **Broker and non-OpenAI-shaped APIs.** Header-based auth is easy; query-param
   keys and request signing (AWS SigV4) are not. v1 supports header auth only and
   says so. **Narrower than it looks in the current code**: the broker always
   sends `Authorization: Bearer` upstream, so an API that wants `x-api-key`
   — Anthropic's own — is not brokerable today either. Per-upstream header
   placement is a small, near-term fix; signing is not.
4. **Version pinning semantics on rotation.** If a `.env` says `@latest` and you
   rotate mid-session, does an open broker session pick up the new value?
   **Settled in the code: no.** A broker route holds the credential it was opened
   with, so a rotation never breaks a running process. The consequence to keep in
   mind is the other one: a rotation performed *because a key leaked* does not
   revoke the sessions already forwarding with it. Closing them is `broker.close`,
   and nothing does it automatically.
5. **Do we need `kw shell`?** A subshell with a broker session already exported is
   the most ergonomic form for interactive work, but a long-lived shell with a live
   session is also the longest-lived exposure window.
6. **Name.** Package registries no longer matter (nothing is published), but for a
   consumer product the *domain*, the App-Store-style searchability, and the
   trademark do. Check `keyward.app` / `keyward.dev` availability and search for
   existing marks in software before committing. Alternates: `hush`, `kestrel`.
7. **Pricing number.** §13.3 guesses $25–39 one-time. Deliberately a guess; M3 is
   sequenced after the first release so it can be replaced with evidence.
9. **Project tokens (§8.1).** A stable, project-scoped broker token would cover the
   IDE-launched dev server, Docker Compose, and `launchd` cases that `kw exec`
   cannot wrap — the single biggest gap in the `.env` story. It needs a stable
   listening port, a revocation UI, and a clear answer to "is this committable?"
   (no: it's machine-local, so committing it breaks every other developer). Deferred
   from v1, but it is the most likely first post-launch feature.
10. **Mac App Store?** Almost certainly not — the sandbox forbids the loopback
   broker talking to arbitrary upstreams, the `PATH` symlink, and the login-item
   daemon. Direct distribution only. Worth confirming rather than assuming.

---

## 13. Distribution, licensing, monetization

Keyward is a **paid consumer app with a fully open codebase**. Those two facts are
in tension in the usual case, and complementary in this one.

### 13.1 Why open source is a feature here, not a concession

For most paid apps, open-sourcing costs you something and buys goodwill. For a
secrets tool it is a *sales argument*, and possibly the decisive one: the entire
value proposition is "trust this process with your API keys." A closed binary
asking for that trust is asking a lot. An auditable one is not.

So the README should lead with it — "every line that touches your keys is on
GitHub; verify the claims in §3 yourself" — rather than treating openness as
licensing housekeeping. Corollaries that follow from taking this seriously:

- **Reproducible builds**, so the notarized `.dmg` is provably the tagged source.
  Hard; worth attempting, and worth saying so honestly if it isn't achieved yet.
- **No telemetry, no analytics, no crash reporter that phones home by default.**
  Any network egress from a secrets daemon undermines the pitch, and reviewers
  *will* run Little Snitch against it. Design for zero background connections
  except the explicit update check, and document that check.
- **No account.** No sign-up, no cloud, nothing to breach. Also removes the entire
  category of "was my vault in the leak?"

### 13.2 Licence choice

The question is what stops someone from cloning the repo, rebranding, and
undercutting you. Three honest options:

| Licence | Can others sell your app? | Cost |
|---|---|---|
| Apache-2.0 | Yes | None, in practice — see below |
| **FSL-1.1-ALv2** *(recommended)* | No, for 2 years; then it becomes Apache-2.0 | Not OSI-"open source"; some people will argue with you about the word |
| Polyform Small Business / BUSL 1.1 | No | Same, plus less familiar to readers |

The practical reality is that nobody meaningfully out-competes a solo consumer Mac
app by rebuilding it from source — the moat is the signed notarized build, the
auto-updater, the icon, and the support address, none of which are in the repo. So
Apache-2.0 would probably be fine.

**FSL** (Functional Source Licence, as used by Sentry) is nonetheless the better
default: it blocks the one scenario you actually care about — a competing paid
product built from your code — costs you essentially nothing with individual users,
and self-converts to Apache-2.0 after two years so the "we'll be truly open
eventually" promise is written into the licence rather than into a blog post.
Note it explicitly in the README, because "source-available" versus "open source"
is a fight you'd rather pre-empt than have.

### 13.3 What is paid, and what is not

**Do not gate security features.** The obvious move is to make broker mode (T1) the
paid tier, since it's the differentiator. Don't: charging for "the mode where your
key can't leak" turns every free user into someone running the less safe
configuration on purpose, and it's a bad look for a security product the first time
someone tweets about it.

Recommended split — pay for the *product*, not for individual protections:

- **Free forever:** the whole codebase. Build it yourself, run it, all tiers, all
  features. Costs you nothing, and is what makes §13.1 credible.
- **Paid (one-time, per major version, ~$25–39):** the signed and notarized build,
  auto-updates, the installer, the icon set, support. This is the Sublime Text /
  many-Mac-indie-app model, and it's honest: you are selling convenience and
  maintenance, and saying so.
- **Possible later subscription:** only if something genuinely recurring appears —
  encrypted multi-device sync, or a team vault. Do not invent one to justify a
  subscription; a local, offline, no-account tool has no recurring cost to pass on
  and users can tell.

**Licence enforcement must be offline.** An Ed25519-signed licence file containing
`{email, purchase_id, issued_at, version_scope}`, verified against a public key
compiled into the app. No activation server, no seat check, no phone-home. This is
trivially bypassable by anyone who reads the source — which is the correct
trade-off, because a secrets daemon that opens a network connection to validate a
licence contradicts everything in §13.1. Sell to the people who will pay; don't
build a DRM system that costs you your central claim.

Payment: Paddle or Lemon Squeezy as merchant of record, so VAT and invoicing aren't
your problem.

### 13.4 Consumer packaging requirements

These are cheap to defer and expensive to retrofit, so treat them as M2 scope:

- **macOS:** Developer ID signing + notarization + stapling (an unsigned `.dmg`
  asking for keychain access is dead on arrival). Sparkle for updates, over HTTPS,
  with the appcast signed. Hardened Runtime. A first-run window that explains, in
  three sentences, what the login item and the `PATH` symlink are for.
- **Windows:** Authenticode signing; without reputation, SmartScreen shows a scary
  red warning to every early user. MSIX or a WiX MSI, not a bare zip.
- **Onboarding is the product.** The single most important screen is the first one:
  paste a key, pick a provider, see it work with `codex` or `claude` in under a
  minute, without reading anything. A secrets tool that requires understanding
  §4 before it does something useful will not convert.
- **A real website with an actual screenshot**, not a GitHub README. `beacon-site`
  already establishes the pattern.

---

## 14. Prior art, and why this exists anyway

- **1Password CLI (`op run`, `op://`)** — the closest thing, and excellent. T0+T2.
  No broker; requires a 1Password subscription; not open source.
- **Doppler / Infisical** — team-oriented, server-backed, T0+T2. Overkill for one
  developer's laptop, and they move your secrets off it.
- **`direnv` + `pass` / `gopass`** — composable and open, but GPG-based, no policy
  layer, no audit, no GUI, and plaintext lands in the environment unconditionally.
- **`git-credential-*` helpers** — the right idea (a broker for one narrow
  protocol), scoped to git only.
- **Cloud IAM / OIDC federation** — strictly better where available. Keyward exists
  because the AI-provider ecosystem has not adopted it.

The gap: nobody offers **T1** for a single developer's machine, and T1 is the only
tier that actually holds up when the thing reading your filesystem is an LLM. That
gap is the reason to build this rather than run `op run`.
