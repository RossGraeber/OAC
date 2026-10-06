// SPDX-License-Identifier: Apache-2.0

//! [`MemoryTransport`]: one endpoint of a [`MemoryNetwork`], implementing
//! `oac_core::transport::Transport` (`spec/interfaces.md` Table 6.4).

use std::sync::{Arc, Mutex, MutexGuard};

use oac_core::health::{HealthState, HealthStatus};
use oac_core::ids::KeyId;
use oac_core::transport::{
    Deadline, Destination, InboundHandler, Payload, PresenceHandler, PublishResult, Subscription,
    Transport, TransportCapabilities, TransportConfiguration, TransportError,
};

use crate::network::{EndpointId, MemoryNetwork, Shared};

/// The configuration `start` takes, wrapped in a `TransportConfiguration`: which network
/// to join.
#[derive(Clone, Debug)]
pub struct MemoryConfiguration {
    network: MemoryNetwork,
}

impl MemoryConfiguration {
    /// Join `network`.
    pub fn new(network: &MemoryNetwork) -> MemoryConfiguration {
        MemoryConfiguration {
            network: network.clone(),
        }
    }

    /// The configuration, wrapped for `Transport::start`.
    pub fn wrap(network: &MemoryNetwork) -> TransportConfiguration {
        TransportConfiguration::new(MemoryConfiguration::new(network))
    }
}

enum Phase {
    NotStarted,
    Running {
        network: MemoryNetwork,
        endpoint: EndpointId,
    },
    ShutDown,
}

/// One endpoint of a [`MemoryNetwork`]. It starts with `start` and stops with `shutdown`
/// (or when dropped); it can be started again after `shutdown`, which is a restart: it
/// joins as a new endpoint, and nothing in flight before the restart reaches it or leaves
/// it afterwards ([IFC-TRN-035]).
pub struct MemoryTransport {
    phase: Mutex<Phase>,
}

impl MemoryTransport {
    /// A transport that is not started yet.
    pub fn new() -> MemoryTransport {
        MemoryTransport {
            phase: Mutex::new(Phase::NotStarted),
        }
    }

    fn phase(&self) -> MutexGuard<'_, Phase> {
        self.phase.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn running(&self) -> Option<(Arc<Shared>, EndpointId)> {
        match &*self.phase() {
            Phase::Running { network, endpoint } => Some((network.shared().clone(), *endpoint)),
            _ => None,
        }
    }
}

impl Default for MemoryTransport {
    fn default() -> Self {
        MemoryTransport::new()
    }
}

impl std::fmt::Debug for MemoryTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryTransport")
            .field("health", &self.health().state)
            .finish()
    }
}

impl Transport for MemoryTransport {
    fn start(
        &self,
        local_device: &KeyId,
        configuration: TransportConfiguration,
    ) -> Result<TransportCapabilities, TransportError> {
        let mut phase = self.phase();
        if matches!(*phase, Phase::Running { .. }) {
            return Err(TransportError::AlreadyStarted);
        }
        let config = configuration
            .into_inner::<MemoryConfiguration>()
            .map_err(|_| {
                TransportError::InvalidConfiguration(
                    "not an in-memory transport configuration".into(),
                )
            })?;
        let endpoint = config.network.shared().join(local_device)?;
        let capabilities = config.network.capabilities();
        *phase = Phase::Running {
            network: config.network,
            endpoint,
        };
        Ok(capabilities)
    }

    fn publish(
        &self,
        destination: &Destination,
        payload: Payload,
        deadline: Deadline,
    ) -> PublishResult {
        match self.running() {
            Some((shared, endpoint)) => {
                shared.publish(endpoint, destination, payload, deadline, false)
            }
            None => PublishResult::NotTaken,
        }
    }

    fn subscribe(
        &self,
        destination: &Destination,
        handler: InboundHandler,
    ) -> Result<Subscription, TransportError> {
        let (shared, endpoint) = self.running().ok_or(TransportError::NotStarted)?;
        let sub = shared.subscribe(endpoint, destination, handler)?;
        Ok(Subscription::new(move || shared.unsubscribe(endpoint, sub)))
    }

    fn send_presence(
        &self,
        destination: &Destination,
        payload: Payload,
        deadline: Deadline,
    ) -> PublishResult {
        match self.running() {
            Some((shared, endpoint)) => {
                shared.publish(endpoint, destination, payload, deadline, true)
            }
            None => PublishResult::NotTaken,
        }
    }

    fn watch_presence(&self, handler: PresenceHandler) -> Result<(), TransportError> {
        let (shared, endpoint) = self.running().ok_or(TransportError::NotStarted)?;
        shared.watch(endpoint, handler)
    }

    fn health(&self) -> HealthStatus {
        // [IFC-TYP-092]: no endpoint number or other native detail in the text.
        match &*self.phase() {
            Phase::NotStarted => HealthStatus::with_detail(HealthState::Unavailable, "not started"),
            Phase::Running { .. } => HealthStatus::new(HealthState::Healthy),
            Phase::ShutDown => HealthStatus::with_detail(HealthState::Unavailable, "shut down"),
        }
    }

    fn shutdown(&self) {
        let previous = std::mem::replace(&mut *self.phase(), Phase::ShutDown);
        match previous {
            Phase::Running { network, endpoint } => network.shared().leave(endpoint),
            Phase::NotStarted => *self.phase() = Phase::NotStarted,
            Phase::ShutDown => {}
        }
    }
}

impl Drop for MemoryTransport {
    fn drop(&mut self) {
        self.shutdown();
    }
}
