use std::time::Duration;
use std::{env, fs};

use base64::prelude::{BASE64_URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use command::r#async::Command;
use warp_core::channel::ChannelState;

use crate::{IsolationPlatformError, WorkloadToken};

/// Typed access to Namespace's build-cache command-line interface.
pub mod spacectl {
    use std::collections::{BTreeMap, BTreeSet};
    use std::ffi::OsString;
    use std::fmt;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::process::ExitStatus;

    use command::r#async::Command;
    use serde::Deserialize;
    use serde::de::DeserializeOwned;
    use thiserror::Error;

    const EXECUTABLE: &str = "spacectl";

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum SpacectlOperation {
        DetectModes,
        Mount,
        MountDetected,
    }
    impl SpacectlOperation {
        fn as_str(self) -> &'static str {
            match self {
                Self::DetectModes => "modes",
                Self::Mount => "mount",
                Self::MountDetected => "mount-detected",
            }
        }
    }

    impl fmt::Display for SpacectlOperation {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(self.as_str())
        }
    }

    /// An error produced while validating, running, or parsing a spacectl cache command.
    #[derive(Debug, Error)]
    pub enum SpacectlError {
        /// The spacectl process could not be started.
        #[error("spacectl cache {operation} command is unavailable")]
        CommandUnavailable {
            /// The attempted cache operation.
            operation: &'static str,
            /// The process-spawn error.
            #[source]
            source: io::Error,
        },
        /// The spacectl process exited unsuccessfully.

        #[error("spacectl cache {operation} command failed with status {status}")]
        CommandFailed {
            /// The attempted cache operation.
            operation: &'static str,
            /// The process exit status.
            status: ExitStatus,
        },
        /// Spacectl returned output that did not match its JSON contract.

        #[error("spacectl cache {operation} returned malformed JSON")]
        MalformedJson {
            /// The attempted cache operation.
            operation: &'static str,
            /// The JSON parsing error.
            #[source]
            source: serde_json::Error,
        },
        /// A cache mode name is empty or cannot be represented in a mode list.

        #[error("spacectl cache mode must not be empty, padded, or contain a comma")]
        InvalidMode,
        /// A mount command did not include any cache modes.

        #[error("at least one spacectl cache mode is required")]
        EmptyModes,
        /// A mount command used an empty or relative cache root.

        #[error("spacectl cache root must be a non-empty absolute path")]
        InvalidCacheRoot,
    }

    /// A validated spacectl cache mode name.
    #[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
    pub struct SpacectlCacheMode(String);

    impl SpacectlCacheMode {
        /// Validates and constructs a cache mode name.
        pub fn new(value: impl Into<String>) -> Result<Self, SpacectlError> {
            let value = value.into();
            if value.is_empty() || value.trim() != value || value.contains(',') {
                return Err(SpacectlError::InvalidMode);
            }
            Ok(Self(value))
        }

        /// Returns the cache mode name.
        pub fn as_str(&self) -> &str {
            &self.0
        }
    }

    #[derive(Debug, Eq, PartialEq)]
    pub(super) struct SpacectlCommand {
        operation: SpacectlOperation,
        arguments: Vec<OsString>,
        cwd: PathBuf,
    }

    impl SpacectlCommand {
        pub(super) fn detect_modes(cwd: &Path) -> Self {
            Self {
                operation: SpacectlOperation::DetectModes,
                arguments: ["cache", "modes", "-o", "json"]
                    .into_iter()
                    .map(OsString::from)
                    .collect(),
                cwd: cwd.to_path_buf(),
            }
        }

        pub(super) fn mount(
            modes: &[SpacectlCacheMode],
            cache_root: &Path,
            cwd: &Path,
        ) -> Result<Self, SpacectlError> {
            let modes = modes
                .iter()
                .map(SpacectlCacheMode::as_str)
                .collect::<BTreeSet<_>>();
            if modes.is_empty() {
                return Err(SpacectlError::EmptyModes);
            }
            validate_cache_root(cache_root)?;

            let mode_argument =
                format!("--mode={}", modes.into_iter().collect::<Vec<_>>().join(","));
            Ok(Self {
                operation: SpacectlOperation::Mount,
                arguments: vec![
                    "cache".into(),
                    "mount".into(),
                    mode_argument.into(),
                    "--dry_run=false".into(),
                    "--cache_root".into(),
                    cache_root.as_os_str().to_owned(),
                    "-o".into(),
                    "json".into(),
                ],
                cwd: cwd.to_path_buf(),
            })
        }

        pub(super) fn mount_detected(cache_root: &Path, cwd: &Path) -> Result<Self, SpacectlError> {
            validate_cache_root(cache_root)?;

            Ok(Self {
                operation: SpacectlOperation::MountDetected,
                arguments: vec![
                    "cache".into(),
                    "mount".into(),
                    "--detect=*".into(),
                    "--dry_run=false".into(),
                    "--cache_root".into(),
                    cache_root.as_os_str().to_owned(),
                    "-o".into(),
                    "json".into(),
                ],
                cwd: cwd.to_path_buf(),
            })
        }

        #[cfg(test)]
        pub(super) fn arguments(&self) -> &[OsString] {
            &self.arguments
        }

        #[cfg(test)]
        pub(super) fn cwd(&self) -> &Path {
            &self.cwd
        }
    }

    /// Executes typed spacectl cache operations.
    #[derive(Debug)]
    pub struct Spacectl {
        executable: PathBuf,
    }

    impl Default for Spacectl {
        fn default() -> Self {
            Self {
                executable: EXECUTABLE.into(),
            }
        }
    }

    impl Spacectl {
        #[cfg(test)]
        pub(super) fn with_executable(executable: impl Into<PathBuf>) -> Self {
            Self {
                executable: executable.into(),
            }
        }

        /// Detects cache modes in `cwd`.
        pub async fn detect_cache_modes(
            &self,
            cwd: &Path,
        ) -> Result<Vec<SpacectlCacheMode>, SpacectlError> {
            let command = SpacectlCommand::detect_modes(cwd);
            let stdout = self.execute(&command).await?;
            parse_detected_modes(&stdout)
        }

        /// Mounts the requested cache modes under an explicit absolute cache root.
        pub async fn mount_cache(
            &self,
            modes: &[SpacectlCacheMode],
            cache_root: &Path,
            cwd: &Path,
        ) -> Result<SpacectlMountResponse, SpacectlError> {
            let command = SpacectlCommand::mount(modes, cache_root, cwd)?;
            let stdout = self.execute(&command).await?;
            parse_mount_response(&stdout)
        }

        /// Detects and mounts every cache mode found in `cwd` under an explicit absolute cache root.
        pub async fn mount_detected_cache(
            &self,
            cache_root: &Path,
            cwd: &Path,
        ) -> Result<SpacectlMountResponse, SpacectlError> {
            let command = SpacectlCommand::mount_detected(cache_root, cwd)?;
            let stdout = self.execute(&command).await?;
            parse_mount_response_for_operation(&stdout, SpacectlOperation::MountDetected)
        }

        async fn execute(&self, command: &SpacectlCommand) -> Result<Vec<u8>, SpacectlError> {
            let output = Command::new(&self.executable)
                .args(&command.arguments)
                .current_dir(&command.cwd)
                .output()
                .await
                .map_err(|source| SpacectlError::CommandUnavailable {
                    operation: command.operation.as_str(),
                    source,
                })?;

            if !output.status.success() {
                log::warn!(
                    "`spacectl cache {}` command failed with status {}",
                    command.operation,
                    output.status
                );
                return Err(SpacectlError::CommandFailed {
                    operation: command.operation.as_str(),
                    status: output.status,
                });
            }

            Ok(output.stdout)
        }
    }

    /// Parsed output from `spacectl cache mount`.
    #[derive(Debug, Eq, PartialEq)]
    pub struct SpacectlMountResponse {
        /// Modes spacectl accepted as input.
        pub input_modes: Vec<SpacectlCacheMode>,
        /// Environment variables required by the mounted caches.
        pub add_envs: BTreeMap<String, String>,
        /// Cache-volume disk usage, when spacectl could determine it.
        pub disk_usage: Option<SpacectlDiskUsage>,
        /// Individual cache mount results.
        pub mounts: Vec<SpacectlMount>,
    }

    /// Cache-volume disk usage reported by spacectl.
    #[derive(Debug, Deserialize, Eq, PartialEq)]
    pub struct SpacectlDiskUsage {
        /// Total cache-volume capacity.
        pub total: String,
        /// Used cache-volume capacity.
        pub used: String,
    }

    /// One cache path mounted by spacectl.
    #[derive(Debug, Eq, PartialEq)]
    pub struct SpacectlMount {
        /// Cache mode that produced this mount.
        pub mode: SpacectlCacheMode,
        /// Path within the cache volume.
        pub cache_path: String,
        /// Destination path mounted into the environment.
        pub mount_path: String,
        /// Whether the cache path already existed.
        pub cache_hit: bool,
    }

    #[derive(Deserialize)]
    struct RawDetectedModes {
        modes: BTreeMap<String, RawDetectedMode>,
    }

    #[derive(Deserialize)]
    struct RawDetectedMode {
        detected: bool,
    }

    #[derive(Deserialize)]
    struct RawMountResponse {
        input: RawMountInput,
        output: RawMountOutput,
    }

    #[derive(Deserialize)]
    struct RawMountInput {
        #[serde(default)]
        modes: Vec<String>,
    }

    #[derive(Deserialize)]
    struct RawMountOutput {
        #[serde(default)]
        add_envs: BTreeMap<String, String>,
        disk_usage: Option<SpacectlDiskUsage>,
        #[serde(default)]
        mounts: Vec<RawMount>,
    }

    #[derive(Deserialize)]
    struct RawMount {
        mode: String,
        cache_path: String,
        mount_path: String,
        cache_hit: bool,
    }

    pub(super) fn parse_detected_modes(
        output: &[u8],
    ) -> Result<Vec<SpacectlCacheMode>, SpacectlError> {
        let output: RawDetectedModes = parse_json(output, SpacectlOperation::DetectModes)?;
        output
            .modes
            .into_iter()
            .filter_map(|(mode, output)| output.detected.then_some(mode))
            .map(SpacectlCacheMode::new)
            .collect()
    }

    pub(super) fn parse_mount_response(
        output: &[u8],
    ) -> Result<SpacectlMountResponse, SpacectlError> {
        parse_mount_response_for_operation(output, SpacectlOperation::Mount)
    }

    fn parse_mount_response_for_operation(
        output: &[u8],
        operation: SpacectlOperation,
    ) -> Result<SpacectlMountResponse, SpacectlError> {
        let output: RawMountResponse = parse_json(output, operation)?;
        let input_modes = output
            .input
            .modes
            .into_iter()
            .map(SpacectlCacheMode::new)
            .collect::<Result<_, _>>()?;
        let mounts = output
            .output
            .mounts
            .into_iter()
            .map(|mount| {
                Ok(SpacectlMount {
                    mode: SpacectlCacheMode::new(mount.mode)?,
                    cache_path: mount.cache_path,
                    mount_path: mount.mount_path,
                    cache_hit: mount.cache_hit,
                })
            })
            .collect::<Result<_, SpacectlError>>()?;

        Ok(SpacectlMountResponse {
            input_modes,
            add_envs: output.output.add_envs,
            disk_usage: output.output.disk_usage,
            mounts,
        })
    }

    fn parse_json<T: DeserializeOwned>(
        output: &[u8],
        operation: SpacectlOperation,
    ) -> Result<T, SpacectlError> {
        serde_json::from_slice(output).map_err(|source| SpacectlError::MalformedJson {
            operation: operation.as_str(),
            source,
        })
    }

    fn validate_cache_root(cache_root: &Path) -> Result<(), SpacectlError> {
        if cache_root.as_os_str().is_empty() || !cache_root.is_absolute() {
            return Err(SpacectlError::InvalidCacheRoot);
        }
        Ok(())
    }
}

/// Detect whether or not we are running in a Namespace instance.
pub fn is_in_namespace_instance() -> bool {
    // For Namespace, match their CLI's logic for detecting a token:
    // https://github.com/namespacelabs/integrations/blob/08d0acd17ce05f8486ec8da329066dd6a12572a0/auth/token.go#L116-L131
    env::var("NSC_TOKEN_FILE").is_ok() || fs::exists("/var/run/nsc/token.json").is_ok_and(|v| v)
}

/// Issue a Namespace workload identity token.
pub async fn issue_workload_token(
    duration: Option<Duration>,
) -> Result<WorkloadToken, IsolationPlatformError> {
    let mut nsc_command = Command::new("nsc");
    nsc_command
        .arg("auth")
        .arg("issue-id-token")
        .arg("--audience")
        .arg(&*ChannelState::workload_audience_url())
        .arg("--output")
        .arg("json");

    if let Some(duration) = duration {
        nsc_command
            .arg("--duration")
            .arg(format!("{}ns", duration.as_nanos()));
    }

    let output =
        nsc_command
            .output()
            .await
            .map_err(|err| IsolationPlatformError::CommandUnavailable {
                command: "nsc".to_owned(),
                source: err,
            })?;

    if !output.status.success() {
        log::warn!(
            "`nsc` command failed with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        return Err(IsolationPlatformError::CommandFailed {
            command: "nsc".to_owned(),
            status: output.status,
        });
    }

    /// JSON output from `nsc auth issue-id-token`.
    #[derive(serde::Deserialize)]
    struct NscTokenOutput {
        id_token: String,
    }

    let token_output = serde_json::from_slice::<NscTokenOutput>(&output.stdout)
        .map_err(|_| anyhow::anyhow!("Unexpected output from `nsc auth issue-id-token`"))?;

    // Namespace ID tokens are JWTs.
    let expires_at = parse_jwt_expiration(&token_output.id_token)?;

    Ok(WorkloadToken {
        token: token_output.id_token,
        expires_at: Some(expires_at),
    })
}

/// Parse the expiration time from a JWT token.
///
/// JWTs have three base64url-encoded parts separated by dots: header.payload.signature.
/// The payload contains an `exp` claim with the Unix timestamp of expiration.
fn parse_jwt_expiration(token: &str) -> Result<DateTime<Utc>, IsolationPlatformError> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(
            anyhow::anyhow!("Invalid JWT format: expected 3 parts, got {}", parts.len()).into(),
        );
    }

    let payload_bytes = BASE64_URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|e| anyhow::anyhow!("Failed to decode JWT payload: {e}"))?;

    #[derive(serde::Deserialize)]
    struct JwtPayload {
        exp: i64,
    }

    let payload: JwtPayload = serde_json::from_slice(&payload_bytes)
        .map_err(|e| anyhow::anyhow!("Failed to parse JWT payload: {e}"))?;

    DateTime::from_timestamp(payload.exp, 0)
        .ok_or_else(|| anyhow::anyhow!("Invalid exp timestamp in JWT: {}", payload.exp).into())
}

#[cfg(test)]
#[path = "namespace_tests.rs"]
mod tests;
