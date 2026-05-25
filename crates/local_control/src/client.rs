//! Blocking client helpers used by the standalone `warpctrl` CLI.
use crate::auth::{CredentialRequest, ScopedCredential};
use crate::discovery::InstanceRecord;
use crate::protocol::{
    ControlError, ControlResponse, ErrorCode, ErrorResponseEnvelope, ExecutionContextProof,
    InvocationContext, RequestEnvelope, ResponseEnvelope,
};

const TERMINAL_PROOF_ID_ENV: &str = "WARPCTRL_TERMINAL_PROOF_ID";
const TERMINAL_SESSION_ID_ENV: &str = "WARPCTRL_TERMINAL_SESSION_ID";
const TERMINAL_PROOF_SECRET_ENV: &str = "WARPCTRL_TERMINAL_PROOF_SECRET";

pub fn send_request(
    instance: &InstanceRecord,
    request: &RequestEnvelope,
) -> Result<ResponseEnvelope, ControlError> {
    let (invocation_context, proof) = invocation_context_from_environment();
    let credential =
        request_credential_with_proof(instance, request.action.kind, invocation_context, proof)?;
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

pub fn request_credential(
    instance: &InstanceRecord,
    action: crate::protocol::ActionKind,
    invocation_context: InvocationContext,
) -> Result<ScopedCredential, ControlError> {
    request_credential_with_proof(instance, action, invocation_context, None)
}

pub fn request_credential_with_proof(
    instance: &InstanceRecord,
    action: crate::protocol::ActionKind,
    invocation_context: InvocationContext,
    execution_context_proof: Option<ExecutionContextProof>,
) -> Result<ScopedCredential, ControlError> {
    let credential_broker = instance.credential_broker.as_ref().ok_or_else(|| {
        ControlError::new(
            ErrorCode::LocalControlDisabled,
            "outside-Warp local control credential broker is disabled for this instance",
        )
    })?;
    let client = reqwest::blocking::Client::new();
    let mut request = CredentialRequest::new(action, invocation_context);
    request.execution_context_proof = execution_context_proof;
    let response = client
        .post(credential_broker.endpoint.credential_url())
        .json(&request)
        .send()
        .map_err(|err| {
            ControlError::with_details(
                ErrorCode::TransportUnavailable,
                "failed to request local-control credential",
                err.to_string(),
            )
        })?;
    let status = response.status();
    let text = response.text().map_err(|err| {
        ControlError::with_details(
            ErrorCode::TransportUnavailable,
            "failed to read local-control credential response",
            err.to_string(),
        )
    })?;
    if let Ok(credential) = serde_json::from_str::<ScopedCredential>(&text) {
        return Ok(credential);
    }
    if let Ok(envelope) = serde_json::from_str::<ErrorResponseEnvelope>(&text) {
        return Err(envelope.error);
    }
    Err(ControlError::with_details(
        ErrorCode::TransportUnavailable,
        format!("local-control credential request failed with HTTP {status}"),
        text,
    ))
}

pub fn invocation_context_from_environment() -> (InvocationContext, Option<ExecutionContextProof>) {
    let proof_id = std::env::var(TERMINAL_PROOF_ID_ENV).ok();
    let terminal_session_id = std::env::var(TERMINAL_SESSION_ID_ENV).ok();
    let proof_secret = std::env::var(TERMINAL_PROOF_SECRET_ENV).ok();
    match (proof_id, terminal_session_id, proof_secret) {
        (Some(proof_id), Some(terminal_session_id), Some(proof_secret)) => (
            InvocationContext::InsideWarp,
            Some(ExecutionContextProof::VerifiedWarpTerminal {
                proof_id,
                terminal_session_id,
                proof_secret,
            }),
        ),
        _ => (InvocationContext::OutsideWarp, None),
    }
}
