//! The vault and the usage log (ARCHITECTURE.md §5).
//!
//! Everything here is written to `vault.json` in the clear. That is deliberate:
//! the document contains no secrets, so it can be backed up or inspected without
//! risk, and a user can read it to see exactly what the app knows about them.
//! Secret *values* live only in the OS keychain and are represented by no type in
//! this crate.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::policy::{Approval, Tier, choose_tier};
use crate::reference::Reference;

/// How a secret is delivered to the program that needs it.
///
/// This is the only classification the product makes, and it is decided by one
/// question: can Keyward stand in front of it? An HTTP API authenticated by a
/// bearer header can be brokered — the program never receives the value. Anything
/// else has to be handed over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Delivery {
    /// Proxied over loopback. `upstream` is where real requests go.
    ///
    /// `base_url_env` names the environment variable that points the client at
    /// the broker — `OPENAI_BASE_URL`, `ANTHROPIC_BASE_URL`, and so on. Most
    /// services have no such convention, and for those it is `None`; callers
    /// fall back to [`derived_base_url_env`] so that there is always some
    /// variable holding the broker's address.
    Brokered {
        upstream: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base_url_env: Option<String>,
    },
    /// Passed to the child process. Database URLs, signing keys, SSH keys.
    Handed,
}

/// The variable Keyward exports for a brokered secret that has no conventional
/// base-URL variable of its own: `gh-test` becomes `KEYWARD_URL_GH_TEST`.
///
/// Seventeen of the twenty services Keyward recognises have no standard base-URL
/// variable, and for all of them brokering used to end the same way: the child
/// held a session token, its SDK sent that token to the real host, and the
/// request failed with `401` for a reason nothing in the output explained. A
/// warning on stderr was not a fix — a coding agent does not read stderr, and the
/// user this product is for does not either.
///
/// Exporting a variable does not by itself make the client use it. What it does
/// is give the agent something concrete to wire up, which `get_reference` then
/// tells it to do by name.
#[must_use]
pub fn derived_base_url_env(name: &str) -> String {
    let mut out = String::from("KEYWARD_URL_");
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_uppercase());
        } else {
            out.push('_');
        }
    }
    out
}

impl Delivery {
    /// The variable that points a client at the broker, conventional or derived.
    ///
    /// `None` for a handed secret, which has no broker to point at.
    #[must_use]
    pub fn base_url_variable(&self, name: &str) -> Option<String> {
        match self {
            Delivery::Brokered { base_url_env, .. } => Some(
                base_url_env
                    .clone()
                    .unwrap_or_else(|| derived_base_url_env(name)),
            ),
            Delivery::Handed => None,
        }
    }

    pub fn is_brokered(&self) -> bool {
        matches!(self, Delivery::Brokered { .. })
    }

    pub fn tier(&self) -> Tier {
        choose_tier(self.is_brokered())
    }
}

/// One secret: a name, a value in the keychain, and how it gets delivered.
///
/// There is no field list and no grouping. One secret is one name and one value,
/// because that is the mental model the product commits to — see DESIGN.md §7
/// ("don't group the list").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Secret {
    /// Slug used in references. Immutable once created: changing it silently
    /// breaks every `.env` pointing at it.
    pub name: String,
    /// What the user sees. Free to change, may be any script.
    pub display: String,
    pub delivery: Delivery,
    /// Enough of the value to recognise it, never enough to use. Computed once,
    /// when the value is stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub masked: Option<String>,
    /// Incremented on rotation so the keychain item name changes and the old
    /// value can be dropped deliberately rather than overwritten in place.
    #[serde(default = "one")]
    pub revision: u32,
    /// Whether to prompt before handing plaintext to a program. Meaningless for
    /// brokered secrets — nothing plaintext moves — so it is ignored there.
    #[serde(default)]
    pub approval: Approval,
}

fn one() -> u32 {
    1
}

impl Secret {
    pub fn reference(&self) -> Result<Reference, crate::reference::ParseRefError> {
        Reference::new(self.name.clone())
    }

    /// Keychain account for the current revision (ARCHITECTURE.md §5).
    pub fn keychain_account(&self) -> String {
        format!("{}/v{}", self.name, self.revision)
    }

    pub fn tier(&self) -> Tier {
        self.delivery.tier()
    }
}

/// Render a value as a hint rather than a secret: a recognisable prefix, the last
/// four characters, and an elision between.
///
/// Values under twelve characters are elided entirely — a mask that reveals most
/// of a short secret is worse than no mask, and this is called on values whose
/// length the caller does not control.
pub fn mask(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() < 12 {
        return "…".repeat(3);
    }
    let head: String = chars.iter().take(7).collect();
    let tail: String = chars
        .iter()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head}…{tail}")
}

/// One recorded use — the "who used it" feature, which is half the product.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Use {
    /// RFC 3339. Stamped by the daemon, which owns the clock.
    pub at: String,
    /// Secret name.
    pub secret: String,
    /// What asked: `codex`, `claude`, `npm run dev`, `stripe-mcp`.
    pub actor: String,
    /// Project directory basename, when the daemon could determine one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// The peer the daemon attested for itself (ARCHITECTURE.md §7), as
    /// [`crate::policy::Caller::describe`] renders it.
    ///
    /// Separate from `actor` because `actor` is whatever the caller said it was —
    /// a program name it passed over the socket, or a `User-Agent` header. That is
    /// the useful label and the untrustworthy one; a log that cannot tell the two
    /// apart cannot answer "did something impersonate `kw`".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller: Option<String>,
    pub tier: Tier,
    #[serde(default = "yes")]
    pub allowed: bool,
}

fn yes() -> bool {
    true
}

/// The metadata document. Secrets are conspicuously absent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vault {
    #[serde(default = "one")]
    pub schema: u32,
    /// Keyed by name, so iteration order is the alphabetical order the list shows.
    #[serde(default)]
    pub secrets: BTreeMap<String, Secret>,
}

impl Vault {
    pub fn get(&self, name: &str) -> Option<&Secret> {
        self.secrets.get(name)
    }

    pub fn resolve(&self, r: &Reference) -> Result<&Secret, ResolveError> {
        self.secrets
            .get(r.name())
            .ok_or_else(|| ResolveError::NoSuchSecret(r.name().to_owned()))
    }

    /// Insert, refusing to overwrite. Callers that mean to replace call
    /// [`Vault::rotate`], so an accidental re-add can never discard a live secret.
    pub fn add(&mut self, secret: Secret) -> Result<(), AddError> {
        if self.secrets.contains_key(&secret.name) {
            return Err(AddError::NameTaken(secret.name));
        }
        self.secrets.insert(secret.name.clone(), secret);
        Ok(())
    }

    /// Bump the revision so the new value lands under a fresh keychain account.
    /// Returns the account name the caller should write to.
    pub fn rotate(&mut self, name: &str, masked: String) -> Result<String, ResolveError> {
        let s = self
            .secrets
            .get_mut(name)
            .ok_or_else(|| ResolveError::NoSuchSecret(name.to_owned()))?;
        s.revision += 1;
        s.masked = Some(masked);
        Ok(s.keychain_account())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResolveError {
    #[error("no secret named `{0}`")]
    NoSuchSecret(String),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AddError {
    #[error("a secret named `{0}` already exists")]
    NameTaken(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stripe() -> Secret {
        Secret {
            name: "stripe".into(),
            display: "Stripe".into(),
            delivery: Delivery::Brokered {
                upstream: "https://api.stripe.com".into(),
                base_url_env: None,
            },
            masked: Some("sk_live_…4f2a".into()),
            revision: 1,
            approval: Approval::Never,
        }
    }

    fn pg() -> Secret {
        Secret {
            name: "pg-prod".into(),
            display: "生产数据库".into(),
            delivery: Delivery::Handed,
            masked: None,
            revision: 1,
            approval: Approval::Ask,
        }
    }

    fn vault() -> Vault {
        let mut v = Vault {
            schema: 1,
            secrets: BTreeMap::new(),
        };
        let _ = v.add(stripe());
        let _ = v.add(pg());
        v
    }

    #[test]
    fn delivery_decides_the_tier_and_nothing_else_does() {
        assert_eq!(stripe().tier(), Tier::Broker);
        assert_eq!(pg().tier(), Tier::Injection);
    }

    #[test]
    fn brokered_secrets_never_disclose_plaintext() {
        assert!(!stripe().tier().discloses_plaintext());
        assert!(pg().tier().discloses_plaintext());
    }

    #[test]
    fn reference_is_a_single_segment() {
        match stripe().reference() {
            Ok(r) => assert_eq!(r.to_string(), "keyward://stripe"),
            Err(e) => unreachable!("{e}"),
        }
    }

    #[test]
    fn resolve_finds_by_name() {
        let v = vault();
        let r = match "keyward://stripe".parse::<Reference>() {
            Ok(r) => r,
            Err(e) => unreachable!("{e}"),
        };
        match v.resolve(&r) {
            Ok(s) => assert_eq!(s.display, "Stripe"),
            Err(e) => unreachable!("{e}"),
        }
    }

    #[test]
    fn resolve_reports_the_missing_name() {
        let v = vault();
        let r = match "keyward://nope".parse::<Reference>() {
            Ok(r) => r,
            Err(e) => unreachable!("{e}"),
        };
        assert_eq!(
            v.resolve(&r),
            Err(ResolveError::NoSuchSecret("nope".into()))
        );
    }

    #[test]
    fn adding_a_duplicate_name_is_refused_not_silently_replaced() {
        let mut v = vault();
        assert_eq!(v.add(stripe()), Err(AddError::NameTaken("stripe".into())));
        // The live secret survives.
        match v.get("stripe") {
            Some(s) => assert_eq!(s.revision, 1),
            None => unreachable!("stripe should still exist"),
        }
    }

    #[test]
    fn rotation_moves_to_a_fresh_keychain_account() {
        let mut v = vault();
        assert_eq!(
            v.rotate("stripe", "sk_live_…9c1b".into()),
            Ok("stripe/v2".to_string()),
            "the new value must not overwrite the old one in place"
        );
        match v.get("stripe") {
            Some(s) => {
                assert_eq!(s.revision, 2);
                assert_eq!(s.masked.as_deref(), Some("sk_live_…9c1b"));
            }
            None => unreachable!("stripe should exist"),
        }
    }

    #[test]
    fn display_name_may_be_any_script_while_the_slug_stays_ascii() {
        assert_eq!(pg().display, "生产数据库");
        assert_eq!(pg().name, "pg-prod");
        match pg().reference() {
            Ok(r) => assert_eq!(r.to_string(), "keyward://pg-prod"),
            Err(e) => unreachable!("{e}"),
        }
    }

    #[test]
    fn masking_never_reveals_a_short_value() {
        assert_eq!(mask("short"), "………");
        assert_eq!(mask("sk_live_51H8xQ2eZ"), "sk_live…Q2eZ");
    }

    #[test]
    fn a_use_written_before_peer_attestation_still_parses() {
        // The usage log is append-only and never rewritten, so every line already
        // on disk predates the `caller` field. Failing to parse them would empty
        // the "who used it" screen on upgrade.
        let old = r#"{"at":"2026-07-25T14:32:11Z","secret":"pg-prod","actor":"psql",
                      "tier":"injection","allowed":true}"#;
        match serde_json::from_str::<Use>(old) {
            Ok(u) => assert_eq!(u.caller, None),
            Err(e) => unreachable!("{e}"),
        }
    }

    #[test]
    fn vault_json_holds_no_values() {
        let json = match serde_json::to_string(&vault()) {
            Ok(j) => j,
            Err(e) => unreachable!("{e}"),
        };
        assert!(
            json.contains("sk_live_…4f2a"),
            "masks are metadata and stay"
        );
        assert!(
            !json.contains("\"value\""),
            "no field named `value` may ever serialise"
        );
    }
}
