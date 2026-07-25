//! The human in the loop (ARCHITECTURE.md §4, §7).
//!
//! `Approval::Ask` is the product's only defence against the careless agent
//! (§3.2 A1), and a defence that dead-ends in an `approval_required` error is not
//! one — it just makes the secret unusable. This is the daemon half: a request
//! parks here, the GUI polls `approval.pending`, a person answers, and the parked
//! request wakes up with the answer.
//!
//! Three properties this file exists to hold:
//!
//! 1. **It fails closed.** A poisoned lock, a timeout, a daemon shutting down —
//!    every path that is not an explicit "allow" resolves to a denial. The cost of
//!    failing closed is a program that has to ask again; the cost of failing open
//!    is a credential handed over because a mutex was poisoned.
//! 2. **It never blocks anything but the asking thread.** The vault lock is not
//!    held while waiting — see the comment in `main.rs::handle` — because the
//!    answer arrives on another connection that needs the daemon to be alive.
//! 3. **A remembered answer is keyed to an attested identity**, never to a pid.

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use keyward_core::Caller;

/// How long a program waits for a person. Sixty seconds is long enough to notice
/// a notification and short enough that a forgotten prompt fails the command
/// rather than hanging a terminal until the user kills it.
pub const TIMEOUT: Duration = Duration::from_secs(60);

/// What a human (or the clock) decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// This request only. The next one asks again.
    Once,
    /// This caller, this secret, until the daemon restarts — the lifetime
    /// `Approval::Ask` documents in `keyward_core::policy`.
    Caller,
    Denied,
    /// Nobody answered. Distinct from `Denied` because the wire codes differ
    /// (§7) and because a timeout is worth a different sentence in the UI.
    TimedOut,
}

impl Outcome {
    pub fn allowed(self) -> bool {
        matches!(self, Outcome::Once | Outcome::Caller)
    }

    /// Wire error code for a refusal (§7). `None` when the request was allowed.
    pub fn code(self) -> Option<&'static str> {
        match self {
            Outcome::Once | Outcome::Caller => None,
            Outcome::Denied => Some("approval_denied"),
            Outcome::TimedOut => Some("approval_timeout"),
        }
    }

    pub fn message(self, secret: &str) -> String {
        match self {
            Outcome::Once | Outcome::Caller => String::new(),
            Outcome::Denied => format!("`{secret}` was not approved"),
            Outcome::TimedOut => {
                format!("nobody answered the approval prompt for `{secret}` in time")
            }
        }
    }

    /// Parse the `decision` field of `approval.resolve`.
    pub fn parse(decision: &str) -> Option<Self> {
        match decision {
            "allow_once" | "allow" => Some(Outcome::Once),
            "allow_caller" | "allow_for_caller" => Some(Outcome::Caller),
            "deny" => Some(Outcome::Denied),
            // `TimedOut` is deliberately unreachable from the wire: only the clock
            // may say the clock ran out.
            _ => None,
        }
    }
}

/// One request waiting for an answer.
struct Pending {
    secret: String,
    /// The attested peer, as the prompt should show it.
    caller: String,
    /// The identity a "remember this" answer is filed under, when the peer was
    /// resolvable at all.
    key: Option<String>,
    actor: String,
    pid: u32,
    purpose: Option<String>,
    project: Option<String>,
    asked_at: Instant,
    answer: Option<Outcome>,
}

/// One row of `approval.pending`, for the GUI. Carries no value and no token.
pub struct Prompt {
    pub id: String,
    pub secret: String,
    pub caller: String,
    pub actor: String,
    pub pid: u32,
    pub purpose: Option<String>,
    pub project: Option<String>,
    /// Seconds left before the request gives up on its own.
    pub expires_in: u64,
}

#[derive(Default)]
struct State {
    waiting: HashMap<String, Pending>,
    /// `(caller key, secret)` pairs a human has already blessed.
    ///
    /// Keyed by secret as well as by caller because "allow `kw`" and "allow `kw`
    /// to read the production database" are different sentences, and the prompt
    /// only ever asked the second one.
    remembered: HashSet<(String, String)>,
    next: u64,
}

#[derive(Clone)]
pub struct Approvals {
    state: Arc<Mutex<State>>,
    answered: Arc<Condvar>,
    timeout: Duration,
}

impl Default for Approvals {
    fn default() -> Self {
        Self::with_timeout(TIMEOUT)
    }
}

impl Approvals {
    pub fn new() -> Self {
        Self::default()
    }

    /// A shorter deadline, so the timeout path is testable without a test that
    /// takes a minute — and a test that takes a minute is a test nobody runs.
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            state: Arc::default(),
            answered: Arc::default(),
            timeout,
        }
    }

    /// Ask, and block until someone answers or [`TIMEOUT`] passes.
    ///
    /// Returns `Outcome::Denied` rather than propagating a lock failure: a caller
    /// that cannot be asked has not been approved.
    pub fn ask(
        &self,
        secret: &str,
        caller: &Caller,
        purpose: Option<&str>,
        project: Option<&str>,
    ) -> Outcome {
        let key = caller.key();
        let id = {
            let Ok(mut state) = self.state.lock() else {
                return Outcome::Denied;
            };
            if let Some(key) = &key
                && state.remembered.contains(&(key.clone(), secret.to_owned()))
            {
                return Outcome::Caller;
            }
            state.next += 1;
            let id = format!("ap_{}", state.next);
            state.waiting.insert(
                id.clone(),
                Pending {
                    secret: secret.to_owned(),
                    caller: caller.describe(),
                    key,
                    actor: caller.actor(),
                    pid: caller.pid,
                    purpose: purpose.map(str::to_owned),
                    project: project.map(str::to_owned),
                    asked_at: Instant::now(),
                    answer: None,
                },
            );
            id
        };

        let outcome = self.wait(&id);

        // Whatever happened, the prompt is over: leaving it listed would show the
        // user a question whose asker has already given up and gone away.
        if let Ok(mut state) = self.state.lock() {
            state.waiting.remove(&id);
        }
        outcome
    }

    fn wait(&self, id: &str) -> Outcome {
        let Ok(state) = self.state.lock() else {
            return Outcome::Denied;
        };
        // `wait_timeout_while` owns the deadline across spurious wakeups, so a
        // noisy condvar cannot extend a 60-second wait into an unbounded one.
        let waited = self
            .answered
            .wait_timeout_while(state, self.timeout, |state| {
                state
                    .waiting
                    .get(id)
                    .is_some_and(|pending| pending.answer.is_none())
            });
        match waited {
            Ok((state, _)) => state
                .waiting
                .get(id)
                .and_then(|pending| pending.answer)
                // The entry vanished while we slept — a shutdown, or a second
                // resolve. Nothing said yes, so the answer is no.
                .unwrap_or(Outcome::TimedOut),
            Err(_) => Outcome::Denied,
        }
    }

    /// Everything a human still has to answer, oldest first: the order they were
    /// asked is the order they should be shown.
    pub fn pending(&self) -> Vec<Prompt> {
        let Ok(state) = self.state.lock() else {
            return Vec::new();
        };
        let mut out: Vec<(Instant, Prompt)> = state
            .waiting
            .iter()
            .filter(|(_, pending)| pending.answer.is_none())
            .map(|(id, pending)| {
                let elapsed = pending.asked_at.elapsed();
                (
                    pending.asked_at,
                    Prompt {
                        id: id.clone(),
                        secret: pending.secret.clone(),
                        caller: pending.caller.clone(),
                        actor: pending.actor.clone(),
                        pid: pending.pid,
                        purpose: pending.purpose.clone(),
                        project: pending.project.clone(),
                        expires_in: self.timeout.saturating_sub(elapsed).as_secs(),
                    },
                )
            })
            .collect();
        out.sort_by_key(|(asked_at, _)| *asked_at);
        out.into_iter().map(|(_, prompt)| prompt).collect()
    }

    /// Record a human's answer. `false` means there was no such prompt — it was
    /// answered already, or it timed out while the window was open.
    pub fn resolve(&self, id: &str, outcome: Outcome) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        let Some(pending) = state.waiting.get_mut(id) else {
            return false;
        };
        if pending.answer.is_some() {
            return false;
        }
        pending.answer = Some(outcome);
        let remember = match (outcome, pending.key.clone()) {
            (Outcome::Caller, Some(key)) => Some((key, pending.secret.clone())),
            // "Allow for this caller" from an unidentified peer degrades to
            // "allow once": there is nothing to file the standing answer against
            // that a different process could not also present tomorrow.
            _ => None,
        };
        if let Some(entry) = remember {
            state.remembered.insert(entry);
        }
        drop(state);
        self.answered.notify_all();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    fn kw() -> Caller {
        Caller {
            pid: 4_242,
            path: Some("/opt/homebrew/bin/kw".into()),
            signing_identity: None,
        }
    }

    fn anonymous() -> Caller {
        Caller {
            pid: 4_242,
            path: None,
            signing_identity: None,
        }
    }

    /// Answer the first prompt that appears, then return its id.
    fn answer(approvals: &Approvals, outcome: Outcome) -> String {
        for _ in 0..2_000 {
            if let Some(prompt) = approvals.pending().into_iter().next() {
                assert!(approvals.resolve(&prompt.id, outcome));
                return prompt.id;
            }
            thread::sleep(Duration::from_millis(1));
        }
        unreachable!("a prompt should have been raised");
    }

    #[test]
    fn a_request_waits_and_then_takes_the_answer_it_was_given() {
        for (decision, expected) in [
            (Outcome::Once, true),
            (Outcome::Caller, true),
            (Outcome::Denied, false),
        ] {
            let approvals = Approvals::new();
            let asking = approvals.clone();
            let asked = thread::spawn(move || asking.ask("pg-prod", &kw(), None, None));
            answer(&approvals, decision);
            match asked.join() {
                Ok(outcome) => {
                    assert_eq!(outcome, decision);
                    assert_eq!(outcome.allowed(), expected);
                }
                Err(_) => unreachable!("the asking thread must not panic"),
            }
        }
    }

    #[test]
    fn an_answered_prompt_leaves_the_queue() {
        let approvals = Approvals::new();
        let asking = approvals.clone();
        let asked = thread::spawn(move || asking.ask("pg-prod", &kw(), None, None));
        let id = answer(&approvals, Outcome::Once);
        let _ = asked.join();
        assert!(approvals.pending().is_empty());
        assert!(
            !approvals.resolve(&id, Outcome::Once),
            "resolving a finished prompt must not appear to work"
        );
    }

    #[test]
    fn allow_for_this_caller_answers_the_next_request_without_asking() {
        let approvals = Approvals::new();
        let asking = approvals.clone();
        let asked = thread::spawn(move || asking.ask("pg-prod", &kw(), None, None));
        answer(&approvals, Outcome::Caller);
        let _ = asked.join();

        // Same caller, same secret: no prompt at all.
        assert_eq!(approvals.ask("pg-prod", &kw(), None, None), Outcome::Caller);
        assert!(approvals.pending().is_empty());
    }

    #[test]
    fn a_standing_answer_does_not_spread_to_another_secret_or_another_caller() {
        let approvals = Approvals::new();
        let asking = approvals.clone();
        let asked = thread::spawn(move || asking.ask("pg-prod", &kw(), None, None));
        answer(&approvals, Outcome::Caller);
        let _ = asked.join();

        let other_secret = approvals.clone();
        let asked = thread::spawn(move || other_secret.ask("stripe", &kw(), None, None));
        assert!(!answer(&approvals, Outcome::Denied).is_empty());
        match asked.join() {
            Ok(outcome) => assert_eq!(outcome, Outcome::Denied),
            Err(_) => unreachable!("the asking thread must not panic"),
        }

        let other_caller = Caller {
            pid: 9,
            path: Some("/tmp/not-kw".into()),
            signing_identity: None,
        };
        let asking = approvals.clone();
        let asked = thread::spawn(move || asking.ask("pg-prod", &other_caller, None, None));
        answer(&approvals, Outcome::Denied);
        match asked.join() {
            Ok(outcome) => assert_eq!(outcome, Outcome::Denied),
            Err(_) => unreachable!("the asking thread must not panic"),
        }
    }

    #[test]
    fn an_unidentified_peer_is_asked_every_time() {
        let approvals = Approvals::new();
        for _ in 0..2 {
            let asking = approvals.clone();
            let asked = thread::spawn(move || asking.ask("pg-prod", &anonymous(), None, None));
            answer(&approvals, Outcome::Caller);
            match asked.join() {
                Ok(outcome) => assert_eq!(outcome, Outcome::Caller),
                Err(_) => unreachable!("the asking thread must not panic"),
            }
        }
    }

    #[test]
    fn the_prompt_carries_what_the_ui_has_to_show() {
        let approvals = Approvals::new();
        let asking = approvals.clone();
        let asked =
            thread::spawn(move || asking.ask("pg-prod", &kw(), Some("npm run dev"), Some("shop")));
        let mut prompt = None;
        for _ in 0..2_000 {
            if let Some(p) = approvals.pending().into_iter().next() {
                prompt = Some(p);
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }
        match prompt {
            Some(p) => {
                assert_eq!(p.secret, "pg-prod");
                assert_eq!(p.caller, "/opt/homebrew/bin/kw");
                assert_eq!(p.actor, "kw");
                assert_eq!(p.pid, 4_242);
                assert_eq!(p.purpose.as_deref(), Some("npm run dev"));
                assert_eq!(p.project.as_deref(), Some("shop"));
                assert!(p.expires_in <= TIMEOUT.as_secs());
                assert!(approvals.resolve(&p.id, Outcome::Denied));
            }
            None => unreachable!("a prompt should have been raised"),
        }
        let _ = asked.join();
    }

    #[test]
    fn an_unanswered_prompt_gives_up_rather_than_hanging_the_program() {
        let approvals = Approvals::with_timeout(Duration::from_millis(60));
        let outcome = approvals.ask("pg-prod", &kw(), None, None);
        assert_eq!(outcome, Outcome::TimedOut);
        assert!(!outcome.allowed(), "silence is not consent");
        assert!(
            approvals.pending().is_empty(),
            "a prompt whose asker has left must not stay on screen"
        );
    }

    #[test]
    fn a_timeout_leaves_nothing_remembered() {
        let approvals = Approvals::with_timeout(Duration::from_millis(60));
        assert_eq!(
            approvals.ask("pg-prod", &kw(), None, None),
            Outcome::TimedOut
        );
        // The second request must be asked again rather than inheriting anything
        // from the first one's silence.
        assert_eq!(
            approvals.ask("pg-prod", &kw(), None, None),
            Outcome::TimedOut
        );
    }

    #[test]
    fn resolving_something_nobody_asked_for_is_refused() {
        let approvals = Approvals::new();
        assert!(!approvals.resolve("ap_nonexistent", Outcome::Once));
    }

    #[test]
    fn only_the_clock_can_produce_a_timeout() {
        assert_eq!(Outcome::parse("allow_once"), Some(Outcome::Once));
        assert_eq!(Outcome::parse("allow_caller"), Some(Outcome::Caller));
        assert_eq!(Outcome::parse("deny"), Some(Outcome::Denied));
        assert_eq!(Outcome::parse("timed_out"), None);
        assert_eq!(Outcome::parse(""), None);
    }

    #[test]
    fn every_refusal_carries_a_wire_code_and_every_approval_carries_none() {
        assert_eq!(Outcome::Once.code(), None);
        assert_eq!(Outcome::Caller.code(), None);
        assert_eq!(Outcome::Denied.code(), Some("approval_denied"));
        assert_eq!(Outcome::TimedOut.code(), Some("approval_timeout"));
        assert!(!Outcome::TimedOut.allowed());
        assert!(!Outcome::Denied.allowed());
    }
}
