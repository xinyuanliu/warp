//! Authenticated scripting grants for local Warp control.
//!
//! Authenticated local-control actions are only eligible for grants that were
//! minted from a verified Warp-managed terminal proof. No standalone external
//! authenticated grant source is modeled here.
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::catalog::PermissionCategory;
use crate::protocol::{ControlError, ErrorCode};

/// Permission scope carried by a verified terminal scripting grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptingScope {
    ReadMetadata,
    ReadUnderlyingData,
    MutateAppState,
    MutateMetadataConfiguration,
    MutateUnderlyingData,
}

impl ScriptingScope {
    pub fn from_permission(permission: PermissionCategory) -> Self {
        match permission {
            PermissionCategory::ReadMetadata => Self::ReadMetadata,
            PermissionCategory::ReadUnderlyingData => Self::ReadUnderlyingData,
            PermissionCategory::MutateAppState => Self::MutateAppState,
            PermissionCategory::MutateMetadataConfiguration => Self::MutateMetadataConfiguration,
            PermissionCategory::MutateUnderlyingData => Self::MutateUnderlyingData,
        }
    }
}

/// How a scripting grant was obtained.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum ScriptingIdentitySource {
    VerifiedWarpTerminal { terminal_session_id: String },
}

/// Authenticated scripting grant attached to a local-control credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptingGrant {
    pub source: ScriptingIdentitySource,
    pub subject: String,
    pub scopes: Vec<ScriptingScope>,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl ScriptingGrant {
    pub fn verified_warp_terminal(
        terminal_session_id: impl Into<String>,
        subject: impl Into<String>,
        scopes: Vec<ScriptingScope>,
        ttl: Duration,
    ) -> Self {
        let issued_at = Utc::now();
        Self {
            source: ScriptingIdentitySource::VerifiedWarpTerminal {
                terminal_session_id: terminal_session_id.into(),
            },
            subject: subject.into(),
            scopes,
            issued_at,
            expires_at: issued_at + ttl,
        }
    }

    pub fn is_expired(&self) -> bool {
        Utc::now() >= self.expires_at
    }

    pub fn has_scope(&self, scope: &ScriptingScope) -> bool {
        self.scopes.contains(scope)
    }

    pub fn verify_scope(&self, scope: ScriptingScope) -> Result<(), ControlError> {
        if self.is_expired() {
            return Err(ControlError::new(
                ErrorCode::UnauthorizedLocalClient,
                "authenticated scripting grant has expired",
            ));
        }
        if !self.has_scope(&scope) {
            return Err(ControlError::new(
                ErrorCode::InsufficientPermissions,
                "authenticated scripting grant does not include the required scope",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "scripting_tests.rs"]
mod tests;
