// SPDX-License-Identifier: Apache-2.0

//! [`PeerConfiguration`]: what `start` is given, in neutral words, and the native
//! configuration built from it inside this crate.
//!
//! Local mode only (C7 §5): peer mode, every listener bound to `127.0.0.1`, no router
//! process. Two ways to find other peers:
//!
//! - **Multicast** (the default, C7 §5 and the G3 verdict): multicast scouting on, at its
//!   default group and `interface: "auto"`, plus gossip. Nothing to configure.
//! - **Rendezvous** (the G3 fallback): scouting off, and a fixed loopback port. The first
//!   peer to start listens on it; every later peer connects to it and learns the others by
//!   gossip. Whether something already accepts connections on the port decides which.
//!
//! TLS listeners and LAN mode are G3's (#64), not this module's.

use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

use oac_core::transport::TransportConfiguration;

/// How a peer finds the other peers of its partition.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Discovery {
    Multicast,
    Rendezvous(u16),
}

/// The configuration of a [`crate::PeerTransport`], passed to `start` wrapped by
/// [`PeerConfiguration::wrap`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerConfiguration {
    discovery: Discovery,
    partition: String,
}

/// The partition label of [`PeerConfiguration::local`].
pub const DEFAULT_PARTITION: &str = "default";

impl Default for PeerConfiguration {
    fn default() -> PeerConfiguration {
        PeerConfiguration::local()
    }
}

/// Which side of a rendezvous a peer starts as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    /// Multicast, or the peer that holds the rendezvous port.
    First,
    /// A peer that connects to the rendezvous port another peer holds.
    Joiner,
}

impl PeerConfiguration {
    /// The zero-configuration local default: loopback listeners, multicast discovery, the
    /// default partition.
    pub fn local() -> PeerConfiguration {
        PeerConfiguration {
            discovery: Discovery::Multicast,
            partition: DEFAULT_PARTITION.to_owned(),
        }
    }

    /// Loopback listeners and a fixed rendezvous port on `127.0.0.1` instead of multicast
    /// discovery (the G3 fallback).
    pub fn rendezvous(port: u16) -> PeerConfiguration {
        PeerConfiguration {
            discovery: Discovery::Rendezvous(port),
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

    /// True when peers are found by multicast, false for a rendezvous port.
    pub fn discovers_by_multicast(&self) -> bool {
        self.discovery == Discovery::Multicast
    }

    /// The configuration, wrapped for `Transport::start`.
    pub fn wrap(self) -> TransportConfiguration {
        TransportConfiguration::new(self)
    }

    /// The roles to try, in order. For a rendezvous, whether something already accepts
    /// connections on the port decides which side is tried first. A listen alone cannot
    /// decide it: some platforms let a second listener bind a port that is in use.
    pub(crate) fn roles(&self) -> &'static [Role] {
        match self.discovery {
            Discovery::Multicast => &[Role::First],
            Discovery::Rendezvous(port) => {
                let a = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
                if TcpStream::connect_timeout(&a, Duration::from_millis(500)).is_ok() {
                    &[Role::Joiner, Role::First]
                } else {
                    &[Role::First, Role::Joiner]
                }
            }
        }
    }

    /// The native configuration for `role`. Errors are neutral text: no address.
    pub(crate) fn native(&self, role: Role) -> Result<zenoh::Config, String> {
        let mut c = zenoh::Config::default();
        let mut set = |k: &str, v: String| {
            c.insert_json5(k, &v)
                .map_err(|_| format!("the native configuration refused the {k} setting"))
        };
        set("mode", r#""peer""#.into())?;
        set("adminspace/enabled", "false".into())?;
        set("scouting/gossip/enabled", "true".into())?;
        // Open returns once scouted peers are connected and their declarations are in
        // (the defaults, stated so a default change is noticed here).
        set("open/return_conditions/connect_scouted", "true".into())?;
        set("open/return_conditions/declares", "true".into())?;
        match (&self.discovery, role) {
            (Discovery::Multicast, _) => {
                set("scouting/multicast/enabled", "true".into())?;
                set("listen/endpoints", r#"["tcp/127.0.0.1:0"]"#.into())?;
            }
            (Discovery::Rendezvous(port), Role::First) => {
                set("scouting/multicast/enabled", "false".into())?;
                set("listen/endpoints", format!(r#"["tcp/127.0.0.1:{port}"]"#))?;
                set("listen/exit_on_failure", "true".into())?;
            }
            (Discovery::Rendezvous(port), Role::Joiner) => {
                set("scouting/multicast/enabled", "false".into())?;
                set("listen/endpoints", r#"["tcp/127.0.0.1:0"]"#.into())?;
                set("connect/endpoints", format!(r#"["tcp/127.0.0.1:{port}"]"#))?;
                set("connect/timeout_ms", "5000".into())?;
                set("connect/exit_on_failure", "true".into())?;
            }
        }
        Ok(c)
    }

    /// The listening endpoint a peer in this configuration may hold, for tests that check
    /// nothing leaks it.
    pub(crate) fn fixed_endpoint(&self) -> Option<String> {
        match self.discovery {
            Discovery::Multicast => None,
            Discovery::Rendezvous(port) => Some(format!("127.0.0.1:{port}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_is_the_default_and_builds() {
        let c = PeerConfiguration::default();
        assert_eq!(c, PeerConfiguration::local());
        assert!(c.discovers_by_multicast());
        assert_eq!(c.partition(), DEFAULT_PARTITION);
        let n = c.native(Role::First).unwrap();
        assert_eq!(n.get_json("mode").unwrap(), r#""peer""#);
        assert_eq!(n.get_json("scouting/multicast/enabled").unwrap(), "true");
        assert!(
            n.get_json("listen/endpoints")
                .unwrap()
                .contains("127.0.0.1:0")
        );
    }

    #[test]
    fn rendezvous_turns_scouting_off_and_has_two_roles() {
        let c = PeerConfiguration::rendezvous(17447).with_partition("t");
        assert!(!c.discovers_by_multicast());
        assert_eq!(c.roles().len(), 2);
        let first = c.native(Role::First).unwrap();
        assert_eq!(
            first.get_json("scouting/multicast/enabled").unwrap(),
            "false"
        );
        assert!(
            first
                .get_json("listen/endpoints")
                .unwrap()
                .contains("127.0.0.1:17447")
        );
        let joiner = c.native(Role::Joiner).unwrap();
        assert!(
            joiner
                .get_json("connect/endpoints")
                .unwrap()
                .contains("127.0.0.1:17447")
        );
        assert_eq!(c.fixed_endpoint().as_deref(), Some("127.0.0.1:17447"));
    }
}
