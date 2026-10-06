// SPDX-License-Identifier: Apache-2.0

//! `HealthStatus` of `spec/interfaces.md` §4.10: what the `health` operation of an adapter
//! (§5.7) or a transport (§6.8) returns.

/// The `state` of a [`HealthStatus`] (`spec/interfaces.md` §4.10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HealthState {
    /// `healthy`.
    Healthy,
    /// `degraded`.
    Degraded,
    /// `unavailable`.
    Unavailable,
}

impl HealthState {
    /// The state's name in `spec/interfaces.md` §4.10.
    pub fn as_str(self) -> &'static str {
        match self {
            HealthState::Healthy => "healthy",
            HealthState::Degraded => "degraded",
            HealthState::Unavailable => "unavailable",
        }
    }
}

/// `HealthStatus` of `spec/interfaces.md` §4.10: a state and an optional diagnostic string
/// for an operator.
///
/// [IFC-TYP-092]: the `detail` never contains a credential, private key material, a
/// transport-native address, a harness-native id or a working directory. No code path
/// reads `detail`; the part that writes it answers for its content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HealthStatus {
    /// `state`.
    pub state: HealthState,
    /// `detail`, a diagnostic string for an operator, when present.
    pub detail: Option<String>,
}

impl HealthStatus {
    /// A status with `state` and no `detail`.
    pub fn new(state: HealthState) -> HealthStatus {
        HealthStatus {
            state,
            detail: None,
        }
    }

    /// A status with `state` and `detail` ([IFC-TYP-092] limits what `detail` may hold).
    pub fn with_detail(state: HealthState, detail: impl Into<String>) -> HealthStatus {
        HealthStatus {
            state,
            detail: Some(detail.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_have_their_spec_names() {
        assert_eq!(HealthState::Healthy.as_str(), "healthy");
        assert_eq!(HealthState::Degraded.as_str(), "degraded");
        assert_eq!(HealthState::Unavailable.as_str(), "unavailable");
        assert_eq!(HealthStatus::new(HealthState::Healthy).detail, None);
        assert_eq!(
            HealthStatus::with_detail(HealthState::Degraded, "x").detail,
            Some("x".to_owned())
        );
    }
}
