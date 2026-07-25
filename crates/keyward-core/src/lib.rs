//! Keyward's platform-independent core: the vault, the `keyward://` reference
//! grammar, and the rules deciding what may leave the daemon.
//!
//! Two invariants define this crate, and both are load-bearing for the security
//! claims in `ARCHITECTURE.md` §3:
//!
//! 1. **It never holds a secret value.** No type here represents plaintext. The
//!    keychain adapter lives in `keywardd`; this crate only describes *where* a
//!    value is and *who* may see it.
//! 2. **It performs no I/O.** So the rules that decide what leaves the daemon are
//!    decidable by `cargo test`, with no keychain, no sockets and no GUI.
//!
//! Everything else in the product — the daemon, the CLI, both desktop apps — is a
//! frontend to the decisions made here.

pub mod model;
pub mod policy;
pub mod reference;

pub use model::{AddError, Delivery, ResolveError, Secret, Use, Vault, derived_base_url_env, mask};
pub use policy::{Approval, Caller, Decision, Denial, Tier, choose_tier, evaluate};
pub use reference::{Found, ParseRefError, Reference, find_all};
