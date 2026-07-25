# Keyward — Use Cases

Companion to `ARCHITECTURE.md`. That document answers *how*; this one answers *for whom,
when, and instead of what*. Written before M0 deliberately: if the scenarios below
don't justify a piece of the design, that piece should be cut before it's built.

Each case states the trigger, what happens today, what Keyward changes, and which
disclosure tier (§4) does the work. Cases are ordered by how strongly they justify
someone downloading and paying — not by how technically interesting they are.

---

## Personas

| | Who | Why they matter |
|---|---|---|
| **P1** | Developer using Claude Code / Codex / Cursor daily, juggling 3–8 provider keys | The core market. Feels the pain weekly. |
| **P2** | Vibe coder — building real things with AI, limited security background | The *growth* market, and the one most likely to leak a key. Will never run `op run`. |
| **P3** | Freelancer / consultant with per-client credentials | Highest willingness to pay; leaking a client's key is a business event. |
| **P4** | Small team (2–8) sharing a repo, not a vault | Drives the reference format. Not a v1 target. |

---

## Tier 1 cases — these are why someone installs

### UC-1 · Stop pasting keys into terminals
**P1, P2 · daily · Tier T1**

**Trigger.** Starting work on a project, or switching a coding agent from Claude to
DeepSeek to Kimi to compare output or cost.

**Today.** Dig the key out of a provider console, an email, a Notes file, or a
password manager. Paste it into the terminal or a `.env`. It's now in
`~/.zsh_history`, in the scrollback, possibly in a screen recording. Repeat every
few days because you forgot which key was which.

**With Keyward.**
```bash
kw exec -- codex                      # the .env holds keyward:// references
```
No key is typed, displayed, or stored in shell history. `kw exec` brokers every
secret it can and says which ones it could not.

*State check:* the CLI path works. The "or one click in the menu bar" half does
not exist — see ARCHITECTURE.md §10.5 and §11.4.

**Why it converts.** This is the daily-use hook. Nobody downloads a security tool
because they feel unsafe; they download it because they're tired. The security is
what makes them keep it. *Design implication: this path has to be the shortest one
in the product, and it must work in under 60 seconds from first launch.*

---

### UC-2 · The agent can't leak what it can't read
**P1, P2 · continuous · Tier T1**

**Trigger.** You point an AI agent at a project and let it run. It reads your files,
debugs a 401, prints environment variables, writes a config.

**Today.** The agent reads `~/.codex/config.toml` or `.env`, and the literal key
enters its context window — which is uploaded to a model provider, retained per
that provider's policy, and possibly shown in a transcript you later share. Nobody
did anything wrong. The key leaked anyway.

**With Keyward.** The child process holds `kws_…`, a loopback token that is
worthless off the machine and dies with the session. The agent can `cat` the config,
run `env`, print the token in a transcript, paste it into a GitHub issue — none of
it matters.

**Why it matters.** This is the case that no other tool covers. `op run` and Doppler
inject the *real* value into the child's environment; the agent is frequently the
parent of that child, so `op run -- env` prints the secret. T1 is the only tier
where the guarantee is technical rather than behavioral. **This is the product's
reason to exist**, and the marketing site's headline.

---

### UC-3 · The agent inlines a key to make something work
**P1, P2 · occasional, expensive · Tier T0**

**Trigger.** The agent is fixing an auth failure. The fastest fix it can see is to
hardcode the key into the source file, or write it into a config it then commits.

**Today.** It works, tests pass, you approve the diff because the diff is 40 lines
and the key looks like noise. Now it's in git history — and rewriting history
doesn't help, because the key must be rotated regardless.

**With Keyward.** The agent writes `keyward://openai`. Deliberately designed
(§8) to be obviously a *name*, so a human skimming a diff registers it instantly,
where `sk-proj-hT8…` reads as noise. `kw scan --staged` as a pre-commit hook fails
on real keys and passes on references.

**Nuance worth being honest about.** This works because the agent is cooperative.
An agent that has the real value and decides inlining it is simpler will do that.
The defense is that under T1 *there is no real value to inline*. T0 and T1 are the
same defense viewed from two sides.

---

### UC-4 · One key, many places; rotation without archaeology
**P1, P3 · quarterly, painful · Tier T0**

**Trigger.** A key leaks, a contract ends, or a provider forces rotation.

**Today.** The key is in `~/.codex/config.toml`, two `.env` files, a shell profile,
a launchd plist, and a Docker Compose file you'd forgotten. You grep, miss one, and
something breaks in three days with an error that doesn't mention credentials.

**With Keyward.** Every location holds `keyward://anthropic`. `kw rotate
anthropic`, paste the new value once, done — the daemon writes the new value under
a fresh keychain account and drops the old one only after the metadata is on disk,
so a crash mid-rotation cannot lose the secret.

**Pinning is cut** (ARCHITECTURE.md §11.5): `@3` parses to a specific error rather
than to an old value, and old revisions are *not* retained. The forensics story
this case imagined — reproduce last Tuesday with the key that was live then — is
gone with it. What remains is the rotation ergonomics, which is the part that
happens quarterly rather than never.

**Corollary that surprises people.** `.env` files become safe to commit. A repo
whose `.env` contains only references is a complete, working configuration for
anyone who has the same secret names locally — and contains nothing sensitive.
This is the single most-liked feature of `op://`-style references in practice, and
the reason the format is worth getting right.

---

### UC-5 · "Why did my bill triple last month?"
**P1, P3 · rare, emotionally decisive · audit log**

**Trigger.** An unexpected invoice, or a provider emailing about anomalous usage.

**Today.** No way to know. Was it the runaway agent loop last Tuesday? A leaked key?
Your own testing? You rotate everything and hope.

**With Keyward.** Every broker session is attributed: which secret, which binary
opened it (attested by the kernel, not self-declared), when, and how many requests.
The app shows a fourteen-day activity strip and a four-column usage table per
secret, plus a live card while a session is forwarding. You can answer the
question.

*State check:* built, and it is the most complete screen in the app. `kw audit
--follow` is not — the log is `uses.jsonl` and the live view is the app's job
(ARCHITECTURE.md §9).

**Why it matters commercially.** This is the *emotional* trigger that makes someone
go looking for a tool. UC-1 is why they keep it; a scare like this is often why they
search in the first place. The audit log is therefore not a power-user feature to
defer — a simple version belongs in the first release.

---

## Tier 2 cases — these justify specific features, not the download

### UC-6 · Per-client credential isolation
**P3 · continuous · T1 + T2**

Freelancer with three clients, each with their own AWS account and LLM billing.
Today: three sets of env vars, careful `AWS_PROFILE` discipline, and a real risk of
running a script against the wrong account — or of client A's key ending up in a
transcript while working on client B's repo.

Keyward: the secret is the isolation unit — one name, one value, one row — and a
broker session is opened per run.
Worth noting this is a *nice-to-have* rather than a differentiator — `direnv` plus
AWS profiles already covers most of it. It matters because P3 has the highest
willingness to pay, not because the alternative is bad.

---

### UC-7 · Tools that must hold the credential themselves
**P1, P3 · weekly · T2**

`terraform apply`, `psql`, `docker login`, `gh`, an SDK with no base-URL override.
These cannot be brokered — the credential isn't an HTTP bearer token, or the client
won't let you redirect it.

```bash
kw exec -- terraform apply
```
There is no separate T2 command (ARCHITECTURE.md §9): `kw exec` tries to broker
every reference and hands over only the ones the daemon refuses to broker, so a
user cannot pick the weaker mode by typing the wrong verb. Values reach the child's
environment and nowhere else — not argv, not a temp file,
not shell history, not disk. **This is strictly weaker than T1** and the UI should
say so when a secret is set to this mode. It exists because the alternative for
these tools is a plaintext file in the home directory, which is worse.

*Design implication:* T2 is load-bearing and can't be cut. But the default for
anything that looks like an LLM API key must remain T1.

---

### UC-8 · Screen sharing, pairing, recording, conference talks
**P1, P2 · situational · T1**

Any moment your terminal is visible to someone else. Today, `cat .env` during a
debugging session is a credential disclosure. Under T1 there is nothing on screen
worth reading. No feature required — it's a consequence of the architecture, but
it's a vivid demo and belongs on the website.

---

### UC-9 · New machine, or handing a project to someone else
**P1, P4 · rare · T0**

Today, setting up a new laptop means hunting down which of a dozen dotfiles held
which credential. With Keyward the vault metadata (`vault.json`, containing zero
secrets) is portable and even commitable; only the values need re-entering, and
they're enumerated for you — `kw list` tells you exactly what's missing rather than
you discovering it one 401 at a time.

---

### UC-10 · Safe agent allowlisting
**P1 · setup-time · T0**

Agent permission systems force a binary choice: allow a command or get prompted
constantly. Users allowlist too much because prompts are annoying.

`kw list`, `kw ref`, `kw status`, `kw scan` and the MCP server are **structurally
incapable** of returning plaintext (§10) — not "won't unless policy allows," but
cannot. So they can be allowlisted without thought.

*State check:* all five are built. The ready-made `.claude/settings.json` snippet
is not shipped anywhere yet, which is the five-line half of a feature whose other
half is done.

---

## What Keyward does *not* solve

Listing these matters as much as the cases above — a security product that
overclaims gets taken apart publicly the first time someone tests it.

- **A determined local attacker running as your user.** Out of scope, permanently
  (§3.2 A5). Anything on your machine as you can debug the daemon or drive the UI.
- **An agent that already has a T2 secret.** `kw exec -- env` prints it. T2 is
  convenience over a plaintext file, not a containment boundary.
- **Non-HTTP credentials at T1.** SSH keys, database passwords, signing keys — T2
  only. v1 also won't broker request-signing schemes like AWS SigV4 (§12.3).
- **Keys you've already leaked.** No detection, no rotation automation, no scanning
  of your git history. Rotate them yourself.
- **Browser logins and human passwords.** Use a password manager. Keyward is for
  machine-to-machine credentials.
- **Team sharing.** No server, no sync, no accounts in v1 (§2 non-goals).

---

## What this list implies for the design

Three things fall out of writing the cases down, and two of them are cuts:

1. **T1 is confirmed as the product.** UC-2 is the only case no competitor covers,
   and UC-1, UC-3, UC-5, UC-8 all resolve to it as a side effect. Everything in the
   design that serves T1 is justified; the broker's hardening details (§6.1) are
   load-bearing, not paranoia.

2. **T3 (reveal) is thinner than the design implies.** No case above needs it
   except "paste into a web console occasionally." It should be a small menu item
   with an approval prompt, not a designed subsystem — and it should *not* appear in
   the three-option control on the add-secret screen, which would give it equal
   visual weight to T1. Move it to a right-click action. **Design change.**

3. **The audit log moves up.** ARCHITECTURE.md treats it as infrastructure; UC-5 makes it
   a first-release user-facing feature with a real UI. **Milestone change: a
   sessions-and-requests view belongs in M2, not "later."**

Open question these cases do not settle: whether P2 (the vibe coder) will ever type
`kw` at all. If the honest answer is no, then the macOS app needs a way to launch an
agent *from the GUI* with a broker session already attached — a "Run project with
Keyward" action — and the CLI stays a P1/P3 feature. That is a significant addition
to M2's scope and should be decided before M2 starts, not during.

---

## Where the code actually is against these cases

ARCHITECTURE.md §11.4 has the per-milestone version; this is the same picture from
the user's side, because "which case works today" is the question that decides what
to build next.

| Case | Works today? |
|---|---|
| UC-1 stop pasting keys | Partly — via `kw exec`, and only for a secret whose delivery is brokered, which nothing in either frontend can currently create |
| UC-2 the agent can't leak what it can't read | The broker is built and correct; the same creation gap makes it unreachable without hand-editing `vault.json` |
| UC-3 agent inlines a key | Yes — references, `kw scan`, and the MCP server all ship |
| UC-4 rotation | Yes for rotation; pinning is cut |
| UC-5 why did my bill triple | Yes, and it is the best screen in the app |
| UC-6 per-client isolation | Partly — no project scoping beyond the recorded project name |
| UC-7 tools that hold the credential | Yes |
| UC-8 screen sharing | Follows from UC-2, with the same caveat |
| UC-9 new machine | Yes — `vault.json` is portable and `kw list` enumerates what is missing |
| UC-10 safe allowlisting | Yes, minus the ready-made config snippet |

**The single change that would move the most rows is a way to mark a secret as
brokered when adding it.** UC-2 is the reason this product exists, the machinery
behind it is finished and tested, and the path from "user has a key" to "that key
is brokered" does not exist. It is a field on the add sheet and a flag on
`kw add`.
