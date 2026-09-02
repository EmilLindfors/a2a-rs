//! The `A2AError` ⇄ `ConnectError` mapping for the ConnectRPC binding.
//!
//! ConnectRPC has sixteen error codes and A2A has its own numbered set, and no
//! table from one to the other is invertible: `MethodNotFound` and
//! `UnsupportedOperation` are both `Unimplemented`, and a refusal with no
//! Connect analogue (`PushNotificationNotSupported`) has nowhere to go but
//! `Internal`. So the A2A code does not round-trip through the Connect code at
//! all. The server attaches the same JSON-RPC error object the JSON-RPC binding
//! would have sent — code, message, typed details — as a Connect error detail,
//! and the client reads it back through the JSON-RPC table. The Connect code is
//! kept as the nearest transport-level category, for a client that is not ours.
//!
//! One module for both directions, for the reason `jsonrpc_wire` is one: two
//! tables kept in step by hand are the bug this replaces.

use connectrpc::{ConnectError, ErrorCode};

use super::jsonrpc_wire::{JsonRpcError, a2a_to_jsonrpc, error_code, jsonrpc_to_a2a};
use crate::domain::A2AError;

/// The `type` of the Connect error detail carrying the A2A error object.
///
/// Connect names a detail by its protobuf message type. A2A's JSON-RPC error
/// is not a protobuf message, so this is a name in this crate's own namespace;
/// the detail's `debug` field carries the object as JSON and `value` is absent.
pub const A2A_ERROR_DETAIL_TYPE: &str = "a2a_rs.JsonRpcError";

/// The nearest Connect category for an A2A error code.
///
/// This is what a client that does not read the detail sees. It is lossy on
/// purpose and never read back by our own client when the detail is present.
pub fn connect_code(a2a_code: i32) -> ErrorCode {
    match a2a_code {
        error_code::TASK_NOT_FOUND => ErrorCode::NotFound,
        error_code::PARSE_ERROR | error_code::INVALID_REQUEST | error_code::INVALID_PARAMS => {
            ErrorCode::InvalidArgument
        }
        error_code::METHOD_NOT_FOUND
        | error_code::UNSUPPORTED_OPERATION
        | error_code::PUSH_NOTIFICATION_NOT_SUPPORTED
        | error_code::CONTENT_TYPE_NOT_SUPPORTED => ErrorCode::Unimplemented,
        error_code::TASK_NOT_CANCELABLE | error_code::EXTENDED_CARD_NOT_CONFIGURED => {
            ErrorCode::FailedPrecondition
        }
        error_code::VERSION_CONFLICT => ErrorCode::Aborted,
        // A context owned by someone else is a refusal, not a fault. As
        // `Internal` it reads as a server bug and invites a retry that will be
        // refused again.
        error_code::CONTEXT_ACCESS_DENIED => ErrorCode::PermissionDenied,
        _ => ErrorCode::Internal,
    }
}

/// Map a domain error onto the wire: the Connect category plus the A2A error
/// object as a detail. [`from_connect_error`] reverses this.
pub fn to_connect_error(err: &A2AError) -> ConnectError {
    let wire = a2a_to_jsonrpc(err);
    let detail = connectrpc::error::ErrorDetail {
        type_url: A2A_ERROR_DETAIL_TYPE.to_string(),
        value: None,
        debug: serde_json::to_value(&wire).ok(),
    };
    ConnectError::new(connect_code(wire.code), wire.message).with_detail(detail)
}

/// Map a Connect error back onto the domain.
///
/// With the A2A detail present this is exact: the variant the server produced
/// comes back, through [`jsonrpc_to_a2a`]. Without it — a server that is not
/// a2a-rs, or a transport-level failure the client library raised itself — the
/// Connect code is read as a category and the result is an [`A2AError::JsonRpc`]
/// carrying the nearest spec code.
pub fn from_connect_error(err: ConnectError) -> A2AError {
    if let Some(wire) = a2a_detail(&err) {
        return jsonrpc_to_a2a(&wire);
    }
    let code = match err.code {
        ErrorCode::NotFound => error_code::TASK_NOT_FOUND,
        ErrorCode::Unimplemented => error_code::METHOD_NOT_FOUND,
        ErrorCode::InvalidArgument => error_code::INVALID_PARAMS,
        ErrorCode::FailedPrecondition => error_code::EXTENDED_CARD_NOT_CONFIGURED,
        ErrorCode::PermissionDenied => error_code::CONTEXT_ACCESS_DENIED,
        _ => error_code::INTERNAL_ERROR,
    };
    A2AError::JsonRpc {
        code,
        message: err.message.unwrap_or_default(),
        data: None,
    }
}

/// The A2A error object a Connect error carries, if it is one of ours.
fn a2a_detail(err: &ConnectError) -> Option<JsonRpcError> {
    err.details
        .iter()
        .filter(|d| d.type_url == A2A_ERROR_DETAIL_TYPE)
        .find_map(|d| serde_json::from_value(d.debug.clone()?).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every variant with a spec code comes back as itself. The message is the
    /// error's `Display` text, prefixed, as on the JSON-RPC wire; what has to
    /// survive is the variant, so that is what is compared. Written as a list
    /// rather than a loop over variants so a new variant that lands in the
    /// catch-all shows up as a missing line here.
    #[test]
    fn every_spec_error_round_trips() {
        let cases = vec![
            A2AError::InvalidRequest("r".into()),
            A2AError::InvalidParams("p".into()),
            A2AError::MethodNotFound("m".into()),
            A2AError::Internal("i".into()),
            A2AError::TaskNotFound("t".into()),
            A2AError::TaskNotCancelable("c".into()),
            A2AError::PushNotificationNotSupported,
            A2AError::UnsupportedOperation("u".into()),
            A2AError::ContentTypeNotSupported("ct".into()),
            A2AError::InvalidAgentResponse("a".into()),
            A2AError::AuthenticatedExtendedCardNotConfigured,
            A2AError::DatabaseError("d".into()),
            A2AError::VersionConflict {
                id: "t1".into(),
                expected: 3,
                actual: 4,
            },
            A2AError::ContextAccessDenied {
                context_id: "ctx-9".into(),
            },
        ];
        for err in cases {
            let back = from_connect_error(to_connect_error(&err));
            assert_eq!(
                back.reason_code(),
                err.reason_code(),
                "{err:?} came back as {back:?}"
            );
            assert_eq!(
                super::super::jsonrpc_wire::a2a_error_code(&back),
                super::super::jsonrpc_wire::a2a_error_code(&err)
            );
        }
    }

    /// The two variants with structured payloads keep them: the numbers a
    /// conflict names and the context a caller was refused.
    #[test]
    fn structured_payloads_survive() {
        let conflict = A2AError::VersionConflict {
            id: "t1".into(),
            expected: 3,
            actual: 4,
        };
        assert!(matches!(
            from_connect_error(to_connect_error(&conflict)),
            A2AError::VersionConflict { id, expected: 3, actual: 4 } if id == "t1"
        ));
        let denied = A2AError::ContextAccessDenied {
            context_id: "ctx-9".into(),
        };
        assert!(matches!(
            from_connect_error(to_connect_error(&denied)),
            A2AError::ContextAccessDenied { context_id } if context_id == "ctx-9"
        ));
    }

    /// The two refusals the issue named: a push refusal is not a server fault,
    /// and an unsupported operation is not a missing method.
    #[test]
    fn a_refusal_is_not_an_internal_error() {
        let push = to_connect_error(&A2AError::PushNotificationNotSupported);
        assert_eq!(push.code, ErrorCode::Unimplemented);
        assert!(matches!(
            from_connect_error(push),
            A2AError::PushNotificationNotSupported
        ));

        let unsupported = to_connect_error(&A2AError::UnsupportedOperation("no".into()));
        let method = to_connect_error(&A2AError::MethodNotFound("no".into()));
        assert_eq!(unsupported.code, method.code, "same Connect category");
        assert!(matches!(
            from_connect_error(unsupported),
            A2AError::UnsupportedOperation(_)
        ));
        assert!(matches!(
            from_connect_error(method),
            A2AError::MethodNotFound(_)
        ));
    }

    /// A validation error keeps its field. The message is the one the JSON-RPC
    /// binding sends, so both transports say the same thing.
    #[test]
    fn a_validation_error_names_the_field_on_the_wire() {
        let err = to_connect_error(&A2AError::ValidationError {
            field: "history_length".into(),
            message: "too large".into(),
        });
        assert_eq!(err.code, ErrorCode::InvalidArgument);
        assert_eq!(err.message.as_deref(), Some("history_length: too large"));
        assert!(matches!(
            from_connect_error(err),
            A2AError::InvalidParams(m) if m == "history_length: too large"
        ));
    }

    /// A Connect error with no detail — a foreign server, or the client library
    /// itself — falls back to the category and keeps the message.
    #[test]
    fn a_foreign_error_falls_back_to_the_category() {
        let err = ConnectError::new(ErrorCode::NotFound, "gone");
        assert!(matches!(
            from_connect_error(err),
            A2AError::JsonRpc { code, message, .. }
                if code == error_code::TASK_NOT_FOUND && message == "gone"
        ));
        let err = ConnectError::new(ErrorCode::Unavailable, "down");
        assert!(matches!(
            from_connect_error(err),
            A2AError::JsonRpc { code, .. } if code == error_code::INTERNAL_ERROR
        ));
    }

    /// The detail is what a Connect client that is not ours sees in `details`.
    #[test]
    fn the_detail_is_the_json_rpc_error_object() {
        let err = to_connect_error(&A2AError::PushNotificationNotSupported);
        let detail = &err.details[0];
        assert_eq!(detail.type_url, A2A_ERROR_DETAIL_TYPE);
        let debug = detail.debug.as_ref().unwrap();
        assert_eq!(debug["code"], error_code::PUSH_NOTIFICATION_NOT_SUPPORTED);
        assert_eq!(
            debug["data"][0]["reason"],
            "PUSH_NOTIFICATION_NOT_SUPPORTED"
        );
    }
}
