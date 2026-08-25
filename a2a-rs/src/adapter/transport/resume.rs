//! Wire details of the a2a-rs streaming-resumption enhancement, shared by the
//! transports that implement it.
//!
//! Resumption is not an A2A v1.0 feature (see
//! [`retry`](super::retry) for the spec note). Over SSE it rides on the W3C
//! standard: the server writes each event's id in the `id:` field and a client
//! sends the last one back as `Last-Event-ID`. A client that ignores `id:` is
//! unaffected, so the JSON-RPC and REST transports emit it for everyone.
//!
//! ConnectRPC is HTTP, so `Last-Event-ID` goes in unchanged as an ordinary
//! request header. Coming back is the problem: there is no `id:` field, and
//! `StreamResponse` carries only its payload oneof. The id travels inside the
//! update event's `metadata` under [`EVENT_ID_KEY`] instead, which changes the
//! payload rather than adding an inert protocol field. So the server stamps it
//! only for a client that asked with [`EVENT_IDS_HEADER`], and our client
//! strips the key again before the event reaches the caller. Everyone else gets
//! the bytes the spec describes.
//!
//! Asking is per-subscription, not per-reconnect: resuming needs the id of the
//! last event received *before* the drop, so ids have to be flowing from the
//! first event.

/// Request header a ConnectRPC client sends to ask for per-event ids in the
/// stream's metadata. Presence is the request; the value is not read.
pub(super) const EVENT_IDS_HEADER: &str = "a2a-rs-event-ids";

/// Metadata key carrying the per-task event id on a ConnectRPC stream response.
///
/// Written as a string rather than a number: the wire type is
/// `google.protobuf.Struct`, whose numbers are doubles, and an id is a `u64`.
pub(super) const EVENT_ID_KEY: &str = "a2a_rs_event_id";

/// Parse the SSE `Last-Event-ID` header into a per-task event id.
///
/// Shared by every server transport: the ConnectRPC adapter reads it from the
/// ConnectRPC request context, the JSON-RPC and REST handlers from the axum
/// request. Both are `http::HeaderMap`.
///
/// A header that is not a plain number reads as absent — a client that sends
/// junk gets a fresh stream from current state, which is what `SubscribeToTask`
/// does without the header at all.
#[cfg(feature = "server")]
pub(super) fn parse_last_event_id(headers: &http::HeaderMap) -> Option<u64> {
    headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
}

/// Whether this ConnectRPC request asked for per-event ids in stream metadata.
#[cfg(feature = "server")]
pub(super) fn wants_event_ids(headers: &http::HeaderMap) -> bool {
    headers.contains_key(EVENT_IDS_HEADER)
}

/// Stamp `id` into an update event's metadata, for a client that asked for it.
///
/// Applied to the domain event on its way out, before the wire mapping, so the
/// id goes through the same `metadata` conversion as everything else.
#[cfg(feature = "server")]
pub(super) fn stamp_event_id(event: &mut crate::port::UpdateEvent, id: u64) {
    use crate::port::UpdateEvent;

    let metadata = match event {
        UpdateEvent::StatusUpdate(event) => &mut event.metadata,
        UpdateEvent::ArtifactUpdate(event) => &mut event.metadata,
    };
    metadata
        .get_or_insert_with(serde_json::Map::new)
        .insert(EVENT_ID_KEY.to_string(), id.to_string().into());
}

/// Take a stamped event id back out of a received update.
///
/// Removes the key: the metadata a caller sees is the metadata the agent wrote.
/// An event whose metadata held nothing else goes back to `None` for the same
/// reason.
///
/// Returns `None` for the initial task snapshot (which has no id to carry), for
/// a server that was not asked to stamp, and for a value that is not a number.
#[cfg(feature = "http-client")]
pub(super) fn take_event_id(item: &mut crate::port::StreamItem) -> Option<u64> {
    use crate::port::StreamItem;

    let metadata = match item {
        StreamItem::StatusUpdate(event) => &mut event.metadata,
        StreamItem::ArtifactUpdate(event) => &mut event.metadata,
        StreamItem::Task(_) => return None,
    };
    let bag = metadata.as_mut()?;
    let stamped = bag.remove(EVENT_ID_KEY);
    if bag.is_empty() {
        *metadata = None;
    }
    stamped?.as_str()?.parse().ok()
}

#[cfg(all(test, feature = "server", feature = "http-client"))]
mod tests {
    use super::*;
    use crate::domain::{TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
    use crate::port::{StreamItem, UpdateEvent};

    fn status_update(metadata: Option<serde_json::Map<String, serde_json::Value>>) -> UpdateEvent {
        UpdateEvent::StatusUpdate(TaskStatusUpdateEvent {
            task_id: "task-1".to_string(),
            context_id: "ctx-1".to_string(),
            kind: "status-update".to_string(),
            status: Default::default(),
            metadata,
        })
    }

    /// What the server stamps is what the client reads back, and the key does
    /// not survive into what the caller sees.
    #[test]
    fn a_stamped_id_round_trips_and_leaves_no_key_behind() {
        let mut event = status_update(None);
        stamp_event_id(&mut event, 42);

        let UpdateEvent::StatusUpdate(event) = event else {
            unreachable!()
        };
        let mut item = StreamItem::StatusUpdate(event);
        assert_eq!(take_event_id(&mut item), Some(42));

        let StreamItem::StatusUpdate(event) = &item else {
            unreachable!()
        };
        assert_eq!(
            event.metadata, None,
            "an event the agent sent no metadata on must not gain a bag"
        );
    }

    /// The agent's own metadata is not ours to edit beyond the one key.
    #[test]
    fn stamping_keeps_the_agents_metadata() {
        let mut bag = serde_json::Map::new();
        bag.insert("step".to_string(), "planning".into());
        let mut event = status_update(Some(bag));
        stamp_event_id(&mut event, 7);

        let UpdateEvent::StatusUpdate(event) = event else {
            unreachable!()
        };
        let mut item = StreamItem::StatusUpdate(event);
        assert_eq!(take_event_id(&mut item), Some(7));

        let StreamItem::StatusUpdate(event) = &item else {
            unreachable!()
        };
        assert_eq!(
            event.metadata.as_ref().and_then(|bag| bag.get("step")),
            Some(&serde_json::Value::from("planning"))
        );
    }

    /// An id above 2^53 is why the key is written as a string: a
    /// `google.protobuf.Struct` number is a double and would round it.
    #[test]
    fn an_id_past_double_precision_survives() {
        let id = (1u64 << 53) + 1;
        let mut event = status_update(None);
        stamp_event_id(&mut event, id);

        let UpdateEvent::StatusUpdate(event) = event else {
            unreachable!()
        };
        assert_eq!(
            take_event_id(&mut StreamItem::StatusUpdate(event)),
            Some(id)
        );
    }

    /// A server that was not asked to stamp sends none, and the client says so
    /// rather than inventing one.
    #[test]
    fn an_unstamped_event_has_no_id() {
        let event = TaskStatusUpdateEvent {
            task_id: "task-1".to_string(),
            context_id: "ctx-1".to_string(),
            kind: "status-update".to_string(),
            status: Default::default(),
            metadata: None,
        };
        assert_eq!(take_event_id(&mut StreamItem::StatusUpdate(event)), None);

        let artifact = TaskArtifactUpdateEvent {
            task_id: "task-1".to_string(),
            context_id: "ctx-1".to_string(),
            kind: "artifact-update".to_string(),
            artifact: Default::default(),
            append: None,
            last_chunk: None,
            metadata: None,
        };
        assert_eq!(
            take_event_id(&mut StreamItem::ArtifactUpdate(artifact)),
            None
        );
    }

    /// The header is the whole request; a client that sends nothing gets the
    /// bytes the spec describes.
    #[test]
    fn event_ids_are_stamped_only_when_asked_for() {
        let mut headers = http::HeaderMap::new();
        assert!(!wants_event_ids(&headers));
        headers.insert(EVENT_IDS_HEADER, http::HeaderValue::from_static("1"));
        assert!(wants_event_ids(&headers));
    }

    /// A `Last-Event-ID` that is not a number is not a resume point. Falling
    /// back to a fresh stream is the spec's own `SubscribeToTask` behavior.
    #[test]
    fn a_last_event_id_header_parses_or_reads_as_absent() {
        let header = |value| {
            let mut headers = http::HeaderMap::new();
            headers.insert("last-event-id", http::HeaderValue::from_static(value));
            parse_last_event_id(&headers)
        };
        assert_eq!(header("17"), Some(17));
        assert_eq!(header(" 17 "), Some(17), "SSE allows surrounding space");
        assert_eq!(header("not-a-number"), None);
        assert_eq!(header(""), None);
        assert_eq!(parse_last_event_id(&http::HeaderMap::new()), None);
    }
}
