// SPDX-License-Identifier: Apache-2.0

//! [`PeerConfiguration`]: what `start` is given, in neutral words, and the native
//! configuration built from it inside this crate.
//!
//! Local mode only (C7 §5): in-process sessions, every listener bound to `127.0.0.1`, no
//! separately run router daemon, and **no multicast scouting and no gossip**. Peers
//! meet at a fixed loopback rendezvous port. The first transport to start listens on it, in
//! Zenoh's router mode inside this process, and relays between the others. Every later
//! transport is a client that connects to it alone, and reconnects to the port if that link
//! drops. No session opens a connection it was not configured with.
//!
//! Why not multicast scouting (the PR #364 review, blocking finding 1): with scouting on, a
//! peer answers and connects out to peers it hears about on any interface, and a plain
//! Zenoh peer on the LAN received envelope frames. Confining it to loopback cannot be
//! guaranteed with the stable configuration on any platform: the scouting interface can be
//! named, but autoconnect has no filter on the locators it is told (a local process can
//! hand a peer a LAN locator by scouting or gossip), and on Linux a socket receives group
//! datagrams any other socket on the host joined on any interface (`IP_MULTICAST_ALL`). So
//! local mode fails closed: scouting off and the fixed rendezvous everywhere. This reverses
//! C7 §5's "multicast scouting on by default" by its own named reversal path (the G3
//! fallback); the dated note in C7 §5 records it. LAN mode and TLS are G3's (#64).

use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

use oac_core::transport::TransportConfiguration;

/// The configuration of a [`crate::PeerTransport`], passed to `start` wrapped by
/// [`PeerConfiguration::wrap`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerConfiguration {
    port: u16,
    partition: String,
}

/// The partition label of [`PeerConfiguration::local`].
pub const DEFAULT_PARTITION: &str = "default";

/// The loopback rendezvous port of [`PeerConfiguration::local`] (the port G3's rendezvous
/// scenario used, `docs/planning/gates/G3-result.md`).
pub const DEFAULT_RENDEZVOUS_PORT: u16 = 17447;

/// How long a blocked `put` waits for a stalled link before Zenoh closes that link, in
/// microseconds (binding document, "Timing").
pub(crate) const BLOCK_BEFORE_CLOSE_MICROS: u64 = 1_000_000;

impl Default for PeerConfiguration {
    fn default() -> PeerConfiguration {
        PeerConfiguration::local()
    }
}

/// Which side of the rendezvous a peer starts as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    /// The peer that holds the rendezvous port.
    First,
    /// A peer that connects to the rendezvous port another peer holds.
    Joiner,
}

impl PeerConfiguration {
    /// The zero-configuration local default: loopback only, the default rendezvous port,
    /// the default partition.
    pub fn local() -> PeerConfiguration {
        PeerConfiguration::rendezvous(DEFAULT_RENDEZVOUS_PORT)
    }

    /// Local mode with the rendezvous on `127.0.0.1:port`.
    pub fn rendezvous(port: u16) -> PeerConfiguration {
        PeerConfiguration {
            port,
            partition: DEFAULT_PARTITION.to_owned(),
        }
    }

    /// The same configuration in partition `label`. Peers in different partitions never
    /// carry each other's payloads. One install uses one partition; tests use a fresh one
    /// per isolated medium.
    pub fn with_partition(mut self, label: impl Into<String>) -> PeerConfiguration {
        self.partition = label.into();
        self
    }

    /// The partition label.
    pub fn partition(&self) -> &str {
        &self.partition
    }

    /// The loopback rendezvous port.
    pub fn rendezvous_port(&self) -> u16 {
        self.port
    }

    /// The configuration, wrapped for `Transport::start`.
    pub fn wrap(self) -> TransportConfiguration {
        TransportConfiguration::new(self)
    }

    /// The roles to try, in order: whether something already accepts connections on the
    /// port decides which side is tried first. A listen alone cannot decide it: some
    /// platforms let a second listener bind a port that is in use.
    pub(crate) fn roles(&self) -> [Role; 2] {
        let a = SocketAddr::from((Ipv4Addr::LOCALHOST, self.port));
        if TcpStream::connect_timeout(&a, Duration::from_millis(500)).is_ok() {
            [Role::Joiner, Role::First]
        } else {
            [Role::First, Role::Joiner]
        }
    }

    /// The loopback port a peer in `role` listens on: the rendezvous port for the first
    /// peer; for a joiner, zero lets Zenoh allocate the ephemeral port atomically.
    pub(crate) fn listen_port(&self, role: Role) -> u16 {
        match role {
            Role::First => self.port,
            Role::Joiner => 0,
        }
    }

    /// The native configuration for `role`, listening on `listen_port`. Errors are neutral
    /// text: no address.
    pub(crate) fn native(&self, role: Role, listen_port: u16) -> Result<zenoh::Config, String> {
        let mut c = zenoh::Config::default();
        let mut set = |k: &str, v: String| {
            c.insert_json5(k, &v)
                .map_err(|_| format!("the native configuration refused the {k} setting"))
        };
        // The peer holding the rendezvous port runs in Zenoh's router mode, in this process,
        // so that it relays between the later peers, which link to it alone (peers do not
        // relay, and without gossip they never link to each other).
        let mode = match role {
            Role::First => r#""router""#,
            Role::Joiner => r#""client""#,
        };
        set("mode", mode.into())?;
        set("adminspace/enabled", "false".into())?;
        // Router mode defaults to automatic timestamps. Sealed samples carry no
        // application time value beside the core frame, including at the relay.
        set("timestamping/enabled", "false".into())?;
        // Loopback only, fail closed: no scouting, no gossip, so no connection is ever made
        // to a locator a peer was told rather than configured with.
        set("scouting/multicast/enabled", "false".into())?;
        set("scouting/gossip/enabled", "false".into())?;
        set(
            "transport/link/tx/queue/congestion_control/block/wait_before_close",
            BLOCK_BEFORE_CLOSE_MICROS.to_string(),
        )?;
        set("open/return_conditions/declares", "true".into())?;
        set(
            "listen/endpoints",
            format!(r#"["tcp/127.0.0.1:{listen_port}"]"#),
        )?;
        set("listen/exit_on_failure", "true".into())?;
        if role == Role::Joiner {
            set(
                "connect/endpoints",
                format!(r#"["tcp/127.0.0.1:{}"]"#, self.port),
            )?;
            set("connect/timeout_ms", "5000".into())?;
            set("connect/exit_on_failure", "true".into())?;
        }
        Ok(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(c: &zenoh::Config, k: &str) -> String {
        c.get_json(k).unwrap()
    }

    #[test]
    fn local_is_the_default_rendezvous_with_nothing_beyond_loopback() {
        let c = PeerConfiguration::default();
        assert_eq!(c, PeerConfiguration::local());
        assert_eq!(c, PeerConfiguration::rendezvous(DEFAULT_RENDEZVOUS_PORT));
        assert_eq!(c.partition(), DEFAULT_PARTITION);
        for role in [Role::First, Role::Joiner] {
            let n = c.native(role, 40000).unwrap();
            let mode = if role == Role::First {
                "router"
            } else {
                "client"
            };
            assert_eq!(json(&n, "mode"), format!(r#""{mode}""#));
            assert_eq!(json(&n, "scouting/multicast/enabled"), "false");
            assert_eq!(json(&n, "scouting/gossip/enabled"), "false");
            assert_eq!(json(&n, "timestamping/enabled"), "false");
            for k in ["listen/endpoints", "connect/endpoints"] {
                let v = json(&n, k);
                let endpoints: Vec<&str> = v.split('"').filter(|s| s.contains('/')).collect();
                assert!(
                    endpoints.iter().all(|e| e.starts_with("tcp/127.0.0.1:")),
                    "{k}: {v}"
                );
            }
        }
    }

    #[test]
    fn the_two_roles() {
        let c = PeerConfiguration::rendezvous(17448).with_partition("t");
        assert_eq!(c.rendezvous_port(), 17448);
        assert_eq!(c.roles().len(), 2);
        let first = c.native(Role::First, c.listen_port(Role::First)).unwrap();
        assert!(json(&first, "listen/endpoints").contains("127.0.0.1:17448"));
        assert_eq!(json(&first, "connect/endpoints"), "[]");
        let joiner = c.native(Role::Joiner, c.listen_port(Role::Joiner)).unwrap();
        assert!(json(&joiner, "listen/endpoints").contains("127.0.0.1:0"));
        assert!(json(&joiner, "connect/endpoints").contains("127.0.0.1:17448"));
    }
}
