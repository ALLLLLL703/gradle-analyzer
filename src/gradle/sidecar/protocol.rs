//! The versioned line-delimited JSON IPC contract spoken between the Rust client and the
//! JVM sidecar.
//!
//! The wire protocol is one JSON object per `\n`-terminated line (see
//! [`crate::gradle::sidecar::framing`]). A session begins with a handshake: the client
//! sends a [`ClientHello`] advertising the protocol-version range and capabilities it
//! supports; the server replies with a [`ServerHello`] naming the [`ClientHello`]-derived
//! `chosen_version` and the negotiated capability set. After the handshake the client
//! sends a [`SidecarRequest`] and the server replies with a [`SidecarResponse`]; an
//! in-flight request may be aborted with a [`CancelRequest`].
//!
//! These are pure data + pure negotiation helpers. No IO happens here; the transport is
//! [`crate::gradle::sidecar::runner::ProcessRunner`] and the orchestration is
//! [`crate::gradle::sidecar::client::SidecarClient`].

use serde::{Deserialize, Serialize};

use crate::gradle::sidecar::model::SidecarModel;

/// The protocol version this build of the analyzer implements.
///
/// Bumped whenever the wire contract changes incompatibly. The handshake rejects any
/// `chosen_version` a client cannot speak (see [`negotiate_version`]).
pub const PROTOCOL_VERSION: u32 = 1;

/// A named capability either side may support, negotiated during the handshake.
///
/// Unknown capability strings are preserved as [`Capability::Other`] rather than rejected,
/// so a newer peer advertising extra capabilities never breaks an older one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum Capability {
    /// The sidecar can produce a full project model import.
    ModelImport,
    /// The sidecar honors mid-request cancellation.
    Cancellation,
    /// The sidecar can resolve `-sources.jar` paths for navigation.
    SourceJars,
    /// An unrecognized capability, preserved verbatim for forward compatibility.
    Other(String),
}

impl From<String> for Capability {
    fn from(value: String) -> Self {
        match value.as_str() {
            "modelImport" => Capability::ModelImport,
            "cancellation" => Capability::Cancellation,
            "sourceJars" => Capability::SourceJars,
            _ => Capability::Other(value),
        }
    }
}

impl From<Capability> for String {
    fn from(value: Capability) -> Self {
        match value {
            Capability::ModelImport => "modelImport".to_string(),
            Capability::Cancellation => "cancellation".to_string(),
            Capability::SourceJars => "sourceJars".to_string(),
            Capability::Other(other) => other,
        }
    }
}

/// The client's opening handshake frame: the version range and capabilities it supports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientHello {
    /// Lowest protocol version the client accepts.
    pub min_version: u32,
    /// Highest protocol version the client accepts.
    pub max_version: u32,
    /// Capabilities the client supports, in preference order.
    pub capabilities: Vec<Capability>,
}

impl ClientHello {
    /// Builds the hello this build sends: the single supported [`PROTOCOL_VERSION`] plus
    /// the given `capabilities`.
    pub fn current(capabilities: Vec<Capability>) -> Self {
        Self {
            min_version: PROTOCOL_VERSION,
            max_version: PROTOCOL_VERSION,
            capabilities,
        }
    }
}

/// The server's handshake reply: the chosen version and negotiated capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerHello {
    /// The protocol version the server selected from the client's advertised range.
    pub chosen_version: u32,
    /// The capabilities both sides agreed on.
    pub capabilities: Vec<Capability>,
}

/// The outcome of a successful handshake: the agreed version and capability set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegotiatedSession {
    /// The protocol version in effect for this session.
    pub version: u32,
    /// The capabilities available for this session.
    pub capabilities: Vec<Capability>,
}

/// A client-to-server request envelope carrying a correlation `id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarRequest {
    /// Correlates this request with its [`SidecarResponse`].
    pub id: u64,
    /// The requested action.
    pub action: RequestAction,
}

/// The action a [`SidecarRequest`] asks the sidecar to perform.
///
/// Only model import exists today; later tasks extend this without breaking the envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum RequestAction {
    /// Import the full Gradle project model.
    ImportModel,
}

/// A request to cancel the in-flight request with the matching `id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelRequest {
    /// The id of the [`SidecarRequest`] to cancel.
    pub id: u64,
}

/// A server-to-client response envelope correlated by `id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarResponse {
    /// The id of the request this response answers.
    pub id: u64,
    /// The response payload (success or a structured error).
    pub outcome: ResponseOutcome,
}

/// The payload of a [`SidecarResponse`]: either a model or a structured sidecar error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "status", content = "body")]
pub enum ResponseOutcome {
    /// The model was imported successfully.
    Model(SidecarModel),
    /// The sidecar reported a structured error (e.g. a Gradle sync failure).
    Error(SidecarErrorBody),
}

/// A structured error the sidecar reports in a [`ResponseOutcome::Error`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarErrorBody {
    /// A stable error code (e.g. `syncFailure`, `staleCache`).
    pub code: String,
    /// A short, log-grade message.
    pub message: String,
}

/// Any frame the server can send, distinguished by a `type` tag.
///
/// Decoding into this single enum lets the client read one line and dispatch on the kind,
/// keeping the framing layer agnostic of message semantics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum ServerMessage {
    /// The server's handshake reply.
    Hello(ServerHello),
    /// A response to a request (boxed: the model payload dwarfs the hello variant).
    Response(Box<SidecarResponse>),
}

/// Any frame the client can send, distinguished by a `type` tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum ClientMessage {
    /// The client's opening handshake.
    Hello(ClientHello),
    /// A request to the server.
    Request(SidecarRequest),
    /// A cancellation of an in-flight request.
    Cancel(CancelRequest),
}

/// Validates a server's `chosen_version` against the client's advertised range.
///
/// Returns the agreed version when it lies within `[client.min_version, client.max_version]`,
/// otherwise `None` so the caller can raise
/// [`crate::gradle::sidecar::SidecarFailure::SchemaMismatch`].
pub fn negotiate_version(client: &ClientHello, server: &ServerHello) -> Option<u32> {
    let chosen = server.chosen_version;
    if chosen >= client.min_version && chosen <= client.max_version {
        Some(chosen)
    } else {
        None
    }
}

/// Computes the negotiated capability set: those the client requested AND the server
/// offered, preserving the client's preference order.
pub fn negotiate_capabilities(
    requested: &[Capability],
    offered: &[Capability],
) -> Vec<Capability> {
    requested
        .iter()
        .filter(|cap| offered.contains(cap))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_version_negotiates_to_the_chosen_value() {
        let client = ClientHello::current(vec![Capability::ModelImport]);
        let server = ServerHello {
            chosen_version: PROTOCOL_VERSION,
            capabilities: vec![Capability::ModelImport],
        };
        assert_eq!(negotiate_version(&client, &server), Some(PROTOCOL_VERSION));
    }

    #[test]
    fn out_of_range_version_fails_negotiation() {
        let client = ClientHello::current(vec![Capability::ModelImport]);
        let server = ServerHello {
            chosen_version: PROTOCOL_VERSION + 1,
            capabilities: vec![],
        };
        assert_eq!(negotiate_version(&client, &server), None);
    }

    #[test]
    fn capability_negotiation_is_the_ordered_intersection() {
        let requested = vec![
            Capability::ModelImport,
            Capability::Cancellation,
            Capability::SourceJars,
        ];
        let offered = vec![Capability::SourceJars, Capability::ModelImport];
        assert_eq!(
            negotiate_capabilities(&requested, &offered),
            vec![Capability::ModelImport, Capability::SourceJars]
        );
    }

    #[test]
    fn unknown_capability_string_round_trips_as_other() {
        let cap = Capability::from("futureThing".to_string());
        assert_eq!(cap, Capability::Other("futureThing".to_string()));
        let back: String = cap.into();
        assert_eq!(back, "futureThing");
    }
}
