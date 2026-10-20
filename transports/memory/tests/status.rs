// SPDX-License-Identifier: Apache-2.0

//! The core's status snapshot over the in-memory transport (G10, #71): the transport's
//! `health` is reported as the transport answered it, through the whole life of the
//! pipelines, and the snapshot of a device with no sessions holds nothing but counters.

use std::sync::Arc;

use oac_core::authorization::{AuthorizationEngine, MemoryDecisionLog};
use oac_core::clock::{Clock, SystemClock};
use oac_core::health::HealthState;
use oac_core::ids::Token;
use oac_core::keys::{DeviceIdentity, DeviceKey};
use oac_core::pipeline::{PipelineConfig, Pipelines};
use oac_core::status::StatusCounts;
use oac_transport_memory::{MemoryConfiguration, MemoryNetwork, MemoryTransport};

fn pipelines(network: &MemoryNetwork) -> Pipelines {
    let identity = DeviceIdentity::new(DeviceKey::generate(), Token::parse("status-a").unwrap());
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let engine =
        AuthorizationEngine::new(&identity, clock.clone(), Box::new(MemoryDecisionLog::new()));
    let net = network.clone();
    Pipelines::new(
        identity,
        engine,
        clock,
        Arc::new(move || net.now()),
        Arc::new(MemoryTransport::new()),
        PipelineConfig::default(),
    )
}

#[test]
fn the_transports_health_is_reported_through_its_life() {
    let network = MemoryNetwork::new();
    let pipes = pipelines(&network);

    let before = pipes.status();
    assert_eq!(before.transport.state, HealthState::Unavailable);
    assert_eq!(before.transport.detail.as_deref(), Some("not started"));

    pipes.start(MemoryConfiguration::wrap(&network)).unwrap();
    let running = pipes.status();
    assert_eq!(running.transport.state, HealthState::Healthy);
    assert_eq!(running.transport.detail, None);
    assert!(running.adapters.is_empty());
    assert!(running.sessions.is_empty());
    assert_eq!(running.counts, StatusCounts::default());

    pipes.shutdown();
    let after = pipes.status();
    assert_eq!(after.transport.state, HealthState::Unavailable);
    assert_eq!(after.transport.detail.as_deref(), Some("shut down"));
}

#[test]
fn a_device_with_nothing_bound_has_a_snapshot_with_no_identifiers_in_it() {
    let network = MemoryNetwork::new();
    let pipes = pipelines(&network);
    pipes.start(MemoryConfiguration::wrap(&network)).unwrap();
    let text = format!("{:?}", pipes.status());
    // The snapshot names no key, no session and no path of any kind.
    assert!(!text.contains(pipes.device().key_id().as_str()), "{text}");
    assert!(!text.contains('/') && !text.contains('\\'), "{text}");
}
