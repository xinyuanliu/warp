use base64::prelude::{BASE64_URL_SAFE_NO_PAD, Engine as _};
use chrono::{TimeZone, Utc};
use std::ffi::OsString;
#[cfg(unix)]
use std::fs;
use std::future::Future;
use std::path::Path;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};

use super::parse_jwt_expiration;
use super::spacectl::{
    Spacectl, SpacectlCacheMode, SpacectlCommand, SpacectlError, parse_detected_modes,
    parse_mount_response,
};

/// Helper to create a JWT token string for testing.
fn make_jwt(payload_json: &str) -> String {
    let header = BASE64_URL_SAFE_NO_PAD.encode(r#"{"alg":"HS256","typ":"JWT"}"#);
    let payload = BASE64_URL_SAFE_NO_PAD.encode(payload_json);
    let signature = BASE64_URL_SAFE_NO_PAD.encode("fake_signature");
    format!("{header}.{payload}.{signature}")
}

fn block_on<F: Future>(future: F) -> F::Output {
    struct ThreadWake(Thread);

    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    let mut future = std::pin::pin!(future);
    let waker = Waker::from(Arc::new(ThreadWake(thread::current())));
    let mut context = Context::from_waker(&waker);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => thread::park(),
        }
    }
}

#[test]
fn test_parse_jwt_expiration_valid() {
    // Unix timestamp for 2024-01-15 12:00:00 UTC.
    let exp_timestamp: i64 = 1705320000;
    let token = make_jwt(&format!(r#"{{"exp":{exp_timestamp},"sub":"user123"}}"#));

    let result = parse_jwt_expiration(&token).unwrap();
    let expected = Utc.timestamp_opt(exp_timestamp, 0).unwrap();

    assert_eq!(result, expected);
}

#[test]
fn test_parse_jwt_expiration_invalid_format_too_few_parts() {
    let token = "header.payload";
    assert!(parse_jwt_expiration(token).is_err());
}

#[test]
fn test_parse_jwt_expiration_invalid_format_too_many_parts() {
    let token = "a.b.c.d";
    assert!(parse_jwt_expiration(token).is_err());
}

#[test]
fn test_parse_jwt_expiration_invalid_json() {
    let header = BASE64_URL_SAFE_NO_PAD.encode(r#"{"alg":"HS256"}"#);
    let payload = BASE64_URL_SAFE_NO_PAD.encode("not valid json");
    let token = format!("{header}.{payload}.signature");

    assert!(parse_jwt_expiration(&token).is_err());
}

#[test]
fn test_parse_jwt_expiration_missing_exp_field() {
    let token = make_jwt(r#"{"sub":"user123","iat":1234567890}"#);
    assert!(parse_jwt_expiration(token.as_str()).is_err());
}

#[test]
fn spacectl_detection_command_has_exact_arguments_and_cwd() {
    let cwd = Path::new("/workspace/repository");
    let command = SpacectlCommand::detect_modes(cwd);

    assert_eq!(
        command.arguments(),
        ["cache", "modes", "-o", "json"].map(OsString::from)
    );
    assert_eq!(command.cwd(), cwd);
}

#[test]
fn spacectl_mount_command_sorts_and_deduplicates_modes() {
    let cwd = Path::new("/workspace/repository");
    let cache_root = Path::new("/cache/build/shared");
    let modes = [
        SpacectlCacheMode::new("rust").unwrap(),
        SpacectlCacheMode::new("go").unwrap(),
        SpacectlCacheMode::new("rust").unwrap(),
    ];

    let command = SpacectlCommand::mount(&modes, cache_root, cwd).unwrap();

    assert_eq!(
        command.arguments(),
        [
            OsString::from("cache"),
            OsString::from("mount"),
            OsString::from("--mode=go,rust"),
            OsString::from("--dry_run=false"),
            OsString::from("--cache_root"),
            cache_root.as_os_str().to_owned(),
            OsString::from("-o"),
            OsString::from("json"),
        ]
    );
    assert_eq!(command.cwd(), cwd);
    assert!(
        command
            .arguments()
            .iter()
            .all(|argument| argument != "NSC_CACHE_PATH")
    );
}

#[test]
fn spacectl_detected_mount_command_has_exact_arguments_and_cwd() {
    let cwd = Path::new("/workspace/repository");
    let cache_root = Path::new("/cache/build/repositories/example");

    let command = SpacectlCommand::mount_detected(cache_root, cwd).unwrap();

    assert_eq!(
        command.arguments(),
        [
            OsString::from("cache"),
            OsString::from("mount"),
            OsString::from("--detect=*"),
            OsString::from("--dry_run=false"),
            OsString::from("--cache_root"),
            cache_root.as_os_str().to_owned(),
            OsString::from("-o"),
            OsString::from("json"),
        ]
    );
    assert_eq!(command.cwd(), cwd);
}

#[test]
fn spacectl_mount_rejects_invalid_inputs_before_spawning() {
    let missing_executable =
        std::env::temp_dir().join(format!("warp-spacectl-missing-{}", std::process::id()));
    let spacectl = Spacectl::with_executable(missing_executable);
    let cwd = std::env::temp_dir();
    let mode = SpacectlCacheMode::new("go").unwrap();

    assert!(matches!(
        block_on(spacectl.mount_cache(&[], &cwd, &cwd)),
        Err(SpacectlError::EmptyModes)
    ));
    assert!(matches!(
        block_on(spacectl.mount_cache(
            std::slice::from_ref(&mode),
            Path::new("relative/cache"),
            &cwd
        )),
        Err(SpacectlError::InvalidCacheRoot)
    ));
    assert!(matches!(
        block_on(spacectl.mount_cache(std::slice::from_ref(&mode), Path::new(""), &cwd)),
        Err(SpacectlError::InvalidCacheRoot)
    ));
    assert!(matches!(
        block_on(spacectl.mount_detected_cache(Path::new("relative/cache"), &cwd)),
        Err(SpacectlError::InvalidCacheRoot)
    ));
    assert!(matches!(
        block_on(spacectl.mount_detected_cache(Path::new(""), &cwd)),
        Err(SpacectlError::InvalidCacheRoot)
    ));
    assert!(matches!(
        SpacectlCacheMode::new(""),
        Err(SpacectlError::InvalidMode)
    ));
    assert!(matches!(
        SpacectlCacheMode::new("go,rust"),
        Err(SpacectlError::InvalidMode)
    ));
}

#[test]
fn spacectl_detection_parses_zero_enabled_modes() {
    let output = br#"{
        "modes": {
            "go": {"detected": false},
            "rust": {"detected": false}
        }
    }"#;

    assert!(parse_detected_modes(output).unwrap().is_empty());
}

#[test]
fn spacectl_detection_returns_every_enabled_mode_in_stable_order() {
    let output = br#"{
        "modes": {
            "rust": {"detected": true},
            "python": {"detected": false},
            "go": {"detected": true}
        }
    }"#;

    let modes = parse_detected_modes(output).unwrap();

    assert_eq!(
        modes
            .iter()
            .map(SpacectlCacheMode::as_str)
            .collect::<Vec<_>>(),
        vec!["go", "rust"]
    );
}

#[test]
fn spacectl_mount_parses_modes_envs_usage_and_mixed_mounts() {
    let output = br#"{
        "input": {
            "modes": ["go", "nix", "rust"]
        },
        "output": {
            "destructive_mode": true,
            "add_envs": {
                "CARGO_HOME": "/cache/build/rust/cargo",
                "GOCACHE": "/cache/build/go/build"
            },
            "disk_usage": {
                "total": "50G",
                "used": "4G"
            },
            "mounts": [
                {
                    "mode": "go",
                    "cache_path": "/cache/build/go/build",
                    "mount_path": "/home/warp/.cache/go-build",
                    "cache_hit": true
                },
                {
                    "mode": "rust",
                    "cache_path": "/cache/build/rust/cargo",
                    "mount_path": "/home/warp/.cargo",
                    "cache_hit": false
                }
            ]
        }
    }"#;

    let response = parse_mount_response(output).unwrap();

    assert_eq!(
        response
            .input_modes
            .iter()
            .map(SpacectlCacheMode::as_str)
            .collect::<Vec<_>>(),
        vec!["go", "nix", "rust"]
    );
    assert_eq!(response.add_envs.len(), 2);
    assert_eq!(
        response.add_envs.get("CARGO_HOME").map(String::as_str),
        Some("/cache/build/rust/cargo")
    );
    assert_eq!(
        response.add_envs.get("GOCACHE").map(String::as_str),
        Some("/cache/build/go/build")
    );
    let disk_usage = response.disk_usage.unwrap();
    assert_eq!(disk_usage.total, "50G");
    assert_eq!(disk_usage.used, "4G");
    assert_eq!(response.mounts.len(), 2);
    assert_eq!(response.mounts[0].mode.as_str(), "go");
    assert_eq!(response.mounts[0].cache_path, "/cache/build/go/build");
    assert_eq!(response.mounts[0].mount_path, "/home/warp/.cache/go-build");
    assert!(response.mounts[0].cache_hit);
    assert_eq!(response.mounts[1].mode.as_str(), "rust");
    assert_eq!(response.mounts[1].cache_path, "/cache/build/rust/cargo");
    assert_eq!(response.mounts[1].mount_path, "/home/warp/.cargo");
    assert!(!response.mounts[1].cache_hit);
    assert!(
        response
            .mounts
            .iter()
            .all(|mount| mount.mode.as_str() != "nix")
    );
}

#[test]
fn spacectl_detected_mount_parses_zero_input_modes() {
    let output = br#"{
        "input": {},
        "output": {"destructive_mode": true}
    }"#;

    let response = parse_mount_response(output).unwrap();

    assert!(response.input_modes.is_empty());
}

#[test]
fn spacectl_mount_accepts_omitted_optional_output_fields() {
    let output = br#"{
        "input": {"modes": ["nix"]},
        "output": {"destructive_mode": true}
    }"#;

    let response = parse_mount_response(output).unwrap();

    assert_eq!(response.input_modes[0].as_str(), "nix");
    assert!(response.add_envs.is_empty());
    assert!(response.disk_usage.is_none());
    assert!(response.mounts.is_empty());
}

#[test]
fn spacectl_parsers_distinguish_malformed_json() {
    assert!(matches!(
        parse_detected_modes(b"not-json"),
        Err(SpacectlError::MalformedJson { .. })
    ));
    assert!(matches!(
        parse_mount_response(br#"{"input":"#),
        Err(SpacectlError::MalformedJson { .. })
    ));
}

#[test]
fn spacectl_detected_mount_distinguishes_an_unavailable_command() {
    let executable =
        std::env::temp_dir().join(format!("warp-spacectl-unavailable-{}", std::process::id()));
    let spacectl = Spacectl::with_executable(executable);

    let cwd = std::env::temp_dir();
    let error = block_on(spacectl.mount_detected_cache(&cwd, &cwd)).unwrap_err();

    assert!(matches!(
        error,
        SpacectlError::CommandUnavailable {
            operation: "mount-detected",
            ..
        }
    ));
}

#[cfg(unix)]
#[test]
fn spacectl_detected_mount_returns_multiple_detected_input_modes() {
    let cwd = std::env::temp_dir().join(format!("warp-spacectl-detected-{}", std::process::id()));
    let _ = fs::remove_dir_all(&cwd);
    fs::create_dir(&cwd).unwrap();
    fs::write(
        cwd.join("cache"),
        "printf '%s\\n' '{\"input\":{\"modes\":[\"go\",\"rust\"]},\"output\":{\"destructive_mode\":true}}'\n",
    )
    .unwrap();
    let spacectl = Spacectl::with_executable("/bin/sh");

    let response = block_on(spacectl.mount_detected_cache(&cwd, &cwd)).unwrap();

    assert_eq!(
        response
            .input_modes
            .iter()
            .map(SpacectlCacheMode::as_str)
            .collect::<Vec<_>>(),
        vec!["go", "rust"]
    );
    fs::remove_dir_all(cwd).unwrap();
}

#[cfg(unix)]
#[test]
fn spacectl_detected_mount_distinguishes_malformed_json() {
    let cwd = std::env::temp_dir().join(format!("warp-spacectl-malformed-{}", std::process::id()));
    let _ = fs::remove_dir_all(&cwd);
    fs::create_dir(&cwd).unwrap();
    fs::write(cwd.join("cache"), "printf 'not-json'\n").unwrap();
    let spacectl = Spacectl::with_executable("/bin/sh");

    let error = block_on(spacectl.mount_detected_cache(&cwd, &cwd)).unwrap_err();

    assert!(matches!(
        error,
        SpacectlError::MalformedJson {
            operation: "mount-detected",
            ..
        }
    ));
    fs::remove_dir_all(cwd).unwrap();
}

#[cfg(unix)]
#[test]
fn spacectl_detected_mount_nonzero_exit_does_not_expose_command_output() {
    let cwd = std::env::temp_dir().join(format!("warp-spacectl-nonzero-{}", std::process::id()));
    let _ = fs::remove_dir_all(&cwd);
    fs::create_dir(&cwd).unwrap();
    fs::write(
        cwd.join("cache"),
        "printf 'arbitrary stdout' >&1\nprintf 'arbitrary stderr' >&2\nexit 23\n",
    )
    .unwrap();
    let spacectl = Spacectl::with_executable("/bin/sh");
    let error = block_on(spacectl.mount_detected_cache(&cwd, &cwd)).unwrap_err();

    match &error {
        SpacectlError::CommandFailed { operation, status } => {
            assert_eq!(*operation, "mount-detected");
            assert_eq!(status.code(), Some(23));
        }
        other => panic!("expected command failure, got {other:?}"),
    }
    let display = error.to_string();
    let debug = format!("{error:?}");
    assert!(!display.contains("arbitrary stdout"));
    assert!(!display.contains("arbitrary stderr"));
    assert!(!debug.contains("arbitrary stdout"));
    assert!(!debug.contains("arbitrary stderr"));
    fs::remove_dir_all(cwd).unwrap();
}
