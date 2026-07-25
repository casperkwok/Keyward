//! Who is on the other end of the socket (ARCHITECTURE.md §7).
//!
//! The usage log is half the product, and a log that records only what the caller
//! said about itself records nothing: `actor` arrives in the request body, so any
//! process on the machine can claim to be `kw`. The kernel knows better, and this
//! module asks it.
//!
//! **No `unsafe` here.** The workspace *forbids* it, which — unlike `deny` — no
//! inner `#[allow]` can lift, so the FFI is borrowed rather than written: `nix`
//! wraps `getsockopt(LOCAL_PEERPID)` and `libproc` wraps `proc_pidpath`. Both are
//! thin, both are the same two syscalls the hand-rolled version would make, and
//! the alternative was to punch a hole in a lint that exists to keep this daemon
//! auditable.
//!
//! Code-signing identity is deliberately absent: §7 says unsigned and unknown
//! binaries are *labelled*, not blocked, so a path is enough to label one, and
//! `SecCodeCopyGuestWithAttributes` has no safe wrapper worth the dependency yet.

use std::os::unix::net::UnixStream;

use keyward_core::Caller;

/// Resolve the peer of one connection. Best effort by design: a peer that cannot
/// be resolved is served and labelled "unidentified", because refusing it would
/// mean a kernel that answered slowly could lock a user out of their own secrets.
#[cfg(target_os = "macos")]
pub fn attest(stream: &UnixStream) -> Caller {
    use nix::sys::socket::{getsockopt, sockopt::LocalPeerPid};

    let pid = getsockopt(stream, LocalPeerPid).unwrap_or(0);
    let path = if pid > 0 {
        libproc::proc_pid::pidpath(pid).ok()
    } else {
        None
    };
    Caller {
        pid: pid.max(0) as u32,
        path,
        signing_identity: None,
    }
}

/// Everywhere else the peer is unresolved rather than wrong.
///
/// `SO_PEERCRED` on Linux and the named-pipe lookup on Windows are §7's job for
/// those platforms; inventing an identity here would put a value in the audit log
/// that nothing checked.
#[cfg(not(target_os = "macos"))]
pub fn attest(_stream: &UnixStream) -> Caller {
    Caller {
        pid: 0,
        path: None,
        signing_identity: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attesting_our_own_socketpair_finds_this_test_binary() {
        let Ok((a, _b)) = UnixStream::pair() else {
            unreachable!("a socketpair is available on every unix");
        };
        let caller = attest(&a);
        if cfg!(target_os = "macos") {
            assert_eq!(caller.pid, std::process::id());
            // Nothing in the daemon may depend on a path being resolvable, so the
            // assertion is about the *shape* of the answer, not its content.
            match caller.path {
                Some(p) => assert!(p.contains("keywardd"), "unexpected peer path {p}"),
                None => unreachable!("proc_pidpath should resolve our own pid"),
            }
            assert_eq!(caller.signing_identity, None);
        }
    }
}
