//! Blocking client helpers used by the standalone `warpctrl` CLI.
use crate::auth::{CredentialRequest, ScopedCredential};
use crate::discovery::InstanceRecord;
use crate::protocol::{
    Action, ActionKind, ControlError, ControlResponse, ErrorCode, ErrorResponseEnvelope,
    InvocationContext, RequestEnvelope, ResponseEnvelope,
};
#[cfg(unix)]
use std::io::{Read as _, Write as _};
#[cfg(unix)]
use std::net::Shutdown;
#[cfg(unix)]
use std::os::unix::net::UnixStream;
#[cfg(unix)]
use std::path::Path;

pub fn send_request(
    instance: &InstanceRecord,
    request: &RequestEnvelope,
) -> Result<ResponseEnvelope, ControlError> {
    instance.validate_local_control_authority()?;
    let credential = request_credential(
        instance,
        request.action.kind,
        InvocationContext::OutsideWarp,
    )?;
    let endpoint = instance.endpoint.as_ref().ok_or_else(|| {
        ControlError::new(
            ErrorCode::LocalControlDisabled,
            "outside-Warp local control endpoint is disabled for this instance",
        )
    })?;
    let client = reqwest::blocking::Client::new();
    let response = client
        .post(endpoint.url())
        .header("Authorization", credential.authorization_value())
        .json(request)
        .send()
        .map_err(|err| {
            ControlError::with_details(
                ErrorCode::TransportUnavailable,
                "failed to send local-control request",
                err.to_string(),
            )
        })?;
    let status = response.status();
    let text = response.text().map_err(|err| {
        ControlError::with_details(
            ErrorCode::TransportUnavailable,
            "failed to read local-control response",
            err.to_string(),
        )
    })?;
    if let Ok(envelope) = serde_json::from_str::<ResponseEnvelope>(&text) {
        if let ControlResponse::Error { error } = &envelope.response {
            return Err(error.clone());
        }
        return Ok(envelope);
    }
    if let Ok(envelope) = serde_json::from_str::<ErrorResponseEnvelope>(&text) {
        return Err(envelope.error);
    }
    Err(ControlError::with_details(
        ErrorCode::TransportUnavailable,
        format!("local-control request failed with HTTP {status}"),
        text,
    ))
}

#[cfg(unix)]
fn request_credential_over_owner_ipc(
    instance: &InstanceRecord,
    request: &CredentialRequest,
) -> Result<String, ControlError> {
    let path = instance.broker_socket_path()?;
    request_credential_over_socket(&path, request)
}

#[cfg(unix)]
fn request_credential_over_socket(
    path: &Path,
    request: &CredentialRequest,
) -> Result<String, ControlError> {
    let mut stream = UnixStream::connect(path).map_err(|err| {
        ControlError::with_details(
            ErrorCode::TransportUnavailable,
            "failed to connect to the owner-authenticated local-control credential broker",
            err.to_string(),
        )
    })?;
    let request = serde_json::to_vec(request).map_err(|err| {
        ControlError::with_details(
            ErrorCode::InvalidRequest,
            "failed to serialize local-control credential request",
            err.to_string(),
        )
    })?;
    stream.write_all(&request).map_err(|err| {
        ControlError::with_details(
            ErrorCode::TransportUnavailable,
            "failed to write local-control credential request",
            err.to_string(),
        )
    })?;
    stream.shutdown(Shutdown::Write).map_err(|err| {
        ControlError::with_details(
            ErrorCode::TransportUnavailable,
            "failed to finish local-control credential request",
            err.to_string(),
        )
    })?;
    let mut response = String::new();
    stream.read_to_string(&mut response).map_err(|err| {
        ControlError::with_details(
            ErrorCode::TransportUnavailable,
            "failed to read local-control credential response",
            err.to_string(),
        )
    })?;
    Ok(response)
}

#[cfg(not(unix))]
fn request_credential_over_owner_ipc(
    _instance: &InstanceRecord,
    _request: &CredentialRequest,
) -> Result<String, ControlError> {
    Err(ControlError::new(
        ErrorCode::LocalControlDisabled,
        "outside-Warp local control requires an owner-authenticated credential broker",
    ))
}

pub fn request_credential(
    instance: &InstanceRecord,
    action: crate::protocol::ActionKind,
    invocation_context: InvocationContext,
) -> Result<ScopedCredential, ControlError> {
    instance.validate_local_control_authority()?;
    let request = CredentialRequest::new(action, invocation_context);
    let text = request_credential_over_owner_ipc(instance, &request)?;
    if let Ok(credential) = serde_json::from_str::<ScopedCredential>(&text) {
        return Ok(credential);
    }
    if let Ok(envelope) = serde_json::from_str::<ErrorResponseEnvelope>(&text) {
        return Err(envelope.error);
    }
    Err(ControlError::with_details(
        ErrorCode::TransportUnavailable,
        "local-control credential broker returned an invalid response",
        text,
    ))
}

pub fn probe_instance(instance: &InstanceRecord) -> Result<(), ControlError> {
    let response = send_request(
        instance,
        &RequestEnvelope::new(Action::new(ActionKind::AppPing)),
    )?;
    validate_probe_response(instance, response)
}

fn validate_probe_response(
    instance: &InstanceRecord,
    response: ResponseEnvelope,
) -> Result<(), ControlError> {
    let ControlResponse::Ok { data } = response.response else {
        return Err(ControlError::new(
            ErrorCode::TransportUnavailable,
            "local-control health probe returned an error response",
        ));
    };
    if data.get("instance_id").and_then(serde_json::Value::as_str)
        != Some(instance.instance_id.0.as_str())
    {
        return Err(ControlError::new(
            ErrorCode::TransportUnavailable,
            "local-control health probe returned a different instance identity",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
