//! What is allowed to leave the daemon (ARCHITECTURE.md §4).
//!
//! No I/O and no secrets, so the rule that matters most in the product is
//! decidable by a unit test on any machine, with no keychain and no sockets.

use serde::{Deserialize, Serialize};

/// How much of a secret a request exposes.
///
/// Ordering is the point: tiers increase strictly in exposure, so a ceiling is
/// checked with `<=`. Do not reorder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// The caller gets `keyward://name`. Nobody sees plaintext.
    Reference,
    /// Traffic is proxied; only the daemon sees plaintext.
    Broker,
    /// The child process receives plaintext in its environment.
    Injection,
    /// A human sees plaintext, via clipboard. A confirmed action, not a mode.
    Reveal,
}

impl Tier {
    /// Whether this tier puts plaintext outside the daemon.
    ///
    /// The distinction the product rests on: Reference and Broker are technical
    /// guarantees, Injection and Reveal are agreements about behaviour.
    pub fn discloses_plaintext(self) -> bool {
        self >= Tier::Injection
    }

    /// The status shown to the user, as a stable key the UI localises.
    ///
    /// A *report*, not a control. Keyward picks the strongest mode available and
    /// says which it got; asking a consumer to choose a security level makes the
    /// product's safety depend on them learning a model they have no reason to
    /// learn (DESIGN.md §7).
    pub fn status_key(self) -> &'static str {
        if self.discloses_plaintext() {
            "shared"
        } else {
            "protected"
        }
    }
}

/// The whole user-facing decision, in one function.
///
/// Brokerable credentials are brokered, everything else is handed over. There is
/// no third case and no user input, which is why a secret can never end up in the
/// weaker mode while the stronger one was available.
pub fn choose_tier(brokerable: bool) -> Tier {
    if brokerable {
        Tier::Broker
    } else {
        Tier::Injection
    }
}

/// Whether to interrupt a human before plaintext moves.
///
/// Two values. `first_use` / `every_use` / `after_idle` were policy depth for an
/// audience that has not asked for it; one switch — ask, or don't — covers the
/// real cases and fits on a settings row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Approval {
    /// No prompt. Correct whenever nothing plaintext is moving.
    #[default]
    Never,
    /// Prompt before handing plaintext to a program, remembering the answer for
    /// that caller until the daemon restarts.
    Ask,
}

/// An attested peer (ARCHITECTURE.md §7). Built by the daemon; described here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    pub pid: u32,
    pub path: Option<String>,
    pub signing_identity: Option<String>,
}

impl Caller {
    /// The label the approval prompt shows.
    ///
    /// A caller with no `signing_identity` is shown as its path and nothing more.
    /// It used to be shown as "<path> (unsigned)", which was a claim nobody had
    /// checked: `peer::attest` does not read code signatures at all, so *every*
    /// caller got the suffix — Keyward's own notarized `kw` included. A warning
    /// that appears on everything teaches people to read past it, which costs
    /// more than saying nothing would.
    ///
    /// When signature checking lands, the missing case becomes a real one and the
    /// word can come back meaning what it says.
    pub fn describe(&self) -> String {
        match (&self.path, &self.signing_identity) {
            (_, Some(id)) => id.clone(),
            (Some(p), None) => p.clone(),
            (None, None) => format!("unidentified process (pid {})", self.pid),
        }
    }

    /// Stable identity for "allow this caller from now on", or `None` when the
    /// peer could not be resolved.
    ///
    /// `None` means the answer is not remembered: a pid is reused within minutes,
    /// so caching against one would hand a later, unrelated process an approval a
    /// human gave to something else.
    pub fn key(&self) -> Option<String> {
        self.signing_identity.clone().or_else(|| self.path.clone())
    }

    /// Best-effort actor name for the usage log: the binary's basename.
    pub fn actor(&self) -> String {
        self.path
            .as_deref()
            .and_then(|p| p.rsplit('/').next())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("pid {}", self.pid))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    /// Prompt unless a prior approval for this caller is still valid; the daemon
    /// owns that cache because it owns the clock.
    NeedsApproval,
    Denied(Denial),
}

/// Why a request was refused, in words a user can act on.
///
/// DESIGN.md §7: no tier vocabulary in anything a person reads. "This secret is
/// delivered at Broker; Injection was requested" is accurate and tells the reader
/// nothing about what to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Denial {
    #[error(
        "this secret is forwarded by Keyward, so no program can receive its value — \
         point the program at Keyward instead of asking for the secret"
    )]
    AboveAvailable { requested: Tier, available: Tier },
}

/// Decide one request.
///
/// `available` is what the secret's delivery mode can actually provide — from
/// [`choose_tier`], never from a user preference.
pub fn evaluate(requested: Tier, available: Tier, approval: Approval) -> Decision {
    if requested > available {
        return Decision::Denied(Denial::AboveAvailable {
            requested,
            available,
        });
    }
    // Tiers that move no plaintext are not worth interrupting a human for.
    // Prompting on them trains users to click through prompts, which is how the
    // prompts that do matter stop working.
    if !requested.discloses_plaintext() {
        return Decision::Allow;
    }
    match approval {
        Approval::Never => Decision::Allow,
        Approval::Ask => Decision::NeedsApproval,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_are_ordered_by_exposure() {
        assert!(Tier::Reference < Tier::Broker);
        assert!(Tier::Broker < Tier::Injection);
        assert!(Tier::Injection < Tier::Reveal);
    }

    #[test]
    fn only_injection_and_reveal_disclose_plaintext() {
        assert!(!Tier::Reference.discloses_plaintext());
        assert!(!Tier::Broker.discloses_plaintext());
        assert!(Tier::Injection.discloses_plaintext());
        assert!(Tier::Reveal.discloses_plaintext());
    }

    #[test]
    fn delivery_alone_decides_what_is_available() {
        assert_eq!(choose_tier(true), Tier::Broker);
        assert_eq!(choose_tier(false), Tier::Injection);
    }

    #[test]
    fn a_brokered_secret_can_never_be_handed_to_a_process() {
        // The headline guarantee. No approval, no setting and no caller can move
        // a brokered value out of the daemon.
        for approval in [Approval::Never, Approval::Ask] {
            assert_eq!(
                evaluate(Tier::Injection, Tier::Broker, approval),
                Decision::Denied(Denial::AboveAvailable {
                    requested: Tier::Injection,
                    available: Tier::Broker
                }),
                "approval {approval:?} must not unlock injection"
            );
            assert_eq!(
                evaluate(Tier::Reveal, Tier::Broker, approval),
                Decision::Denied(Denial::AboveAvailable {
                    requested: Tier::Reveal,
                    available: Tier::Broker
                })
            );
        }
    }

    #[test]
    fn brokering_and_references_never_prompt() {
        assert_eq!(
            evaluate(Tier::Reference, Tier::Broker, Approval::Ask),
            Decision::Allow
        );
        assert_eq!(
            evaluate(Tier::Broker, Tier::Broker, Approval::Ask),
            Decision::Allow
        );
    }

    #[test]
    fn handed_secrets_respect_the_ask_switch() {
        assert_eq!(
            evaluate(Tier::Injection, Tier::Injection, Approval::Ask),
            Decision::NeedsApproval
        );
        assert_eq!(
            evaluate(Tier::Injection, Tier::Injection, Approval::Never),
            Decision::Allow
        );
    }

    #[test]
    fn an_unverified_caller_is_shown_as_its_path_without_a_verdict() {
        let c = Caller {
            pid: 42,
            path: Some("/opt/homebrew/bin/terraform".into()),
            signing_identity: None,
        };
        // Not "… (unsigned)". Nothing checked the signature, and a suffix that
        // lands on every caller — including Keyward's own signed binaries — is a
        // warning people learn to skip.
        assert_eq!(c.describe(), "/opt/homebrew/bin/terraform");
        assert_eq!(c.actor(), "terraform");
    }

    #[test]
    fn actor_falls_back_to_a_pid_rather_than_lying() {
        let c = Caller {
            pid: 991,
            path: None,
            signing_identity: None,
        };
        assert_eq!(c.actor(), "pid 991");
    }

    #[test]
    fn an_unresolved_peer_has_no_identity_to_remember_an_approval_against() {
        let unknown = Caller {
            pid: 991,
            path: None,
            signing_identity: None,
        };
        assert_eq!(
            unknown.key(),
            None,
            "a pid is reused, so it must not key a standing approval"
        );
        let resolved = Caller {
            pid: 991,
            path: Some("/opt/homebrew/bin/kw".into()),
            signing_identity: None,
        };
        assert_eq!(resolved.key().as_deref(), Some("/opt/homebrew/bin/kw"));
    }
}
