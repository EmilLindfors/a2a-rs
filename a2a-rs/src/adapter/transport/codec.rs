//! Shared client-side wire decoding helpers.
//!
//! Both transport client adapters (ConnectRPC's `HttpClient` and the JSON-RPC
//! `JsonRpcClient`) receive the same generated [`StreamResponse`] union on a
//! subscription and must map it to the protocol-neutral [`StreamItem`] the
//! [`Transport`](crate::port::Transport) port yields. Keeping that mapping here
//! ensures both directions agree.

use crate::domain::generated::{StreamResponse, stream_response};
use crate::port::StreamItem;

/// Map a wire [`StreamResponse`] (tag-free field-presence union) onto the
/// protocol-neutral [`StreamItem`]. Returns `None` for an empty/unrecognized
/// payload.
pub fn stream_response_to_item(resp: StreamResponse) -> Option<StreamItem> {
    match resp.payload {
        Some(stream_response::Payload::Task(task)) => Some(StreamItem::Task(*task)),
        Some(stream_response::Payload::StatusUpdate(update)) => {
            Some(StreamItem::StatusUpdate((*update).into()))
        }
        Some(stream_response::Payload::ArtifactUpdate(update)) => {
            Some(StreamItem::ArtifactUpdate((*update).into()))
        }
        _ => None,
    }
}
