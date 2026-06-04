//! Incremental tool-call assembly for streaming LLM responses.
//!
//! Providers stream a tool call as a sequence of
//! [`ToolCallChunk`](super::LlmStreamEvent::ToolCallChunk)s (an id, an optional
//! name, and a fragment of the JSON arguments) followed by a finalized
//! [`ToolCall`]. [`ToolCallAccumulator`] folds those chunks — keyed by call id,
//! so interleaved calls stay separate — into running [`PartialToolCall`]s that a
//! UI can render live, and reconciles the authoritative final call.

use super::ToolCall;

/// A tool call assembled so far from streamed chunks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PartialToolCall {
    /// Provider-assigned call id (stable across this call's chunks).
    pub id: String,
    /// Function name, once a chunk has carried it.
    pub name: Option<String>,
    /// JSON arguments accumulated so far (may be partial/unparseable mid-stream).
    pub arguments: String,
    /// Set once a finalized [`ToolCall`] has reconciled this entry.
    pub complete: bool,
}

impl PartialToolCall {
    fn to_tool_call(&self) -> ToolCall {
        ToolCall {
            id: self.id.clone(),
            name: self.name.clone().unwrap_or_default(),
            arguments: self.arguments.clone(),
        }
    }
}

/// Folds streamed [`ToolCallChunk`](super::LlmStreamEvent::ToolCallChunk)s into
/// complete [`ToolCall`]s, preserving first-seen order.
#[derive(Debug, Default)]
pub struct ToolCallAccumulator {
    calls: Vec<PartialToolCall>,
}

impl ToolCallAccumulator {
    /// Create an empty accumulator.
    pub fn new() -> Self {
        Self::default()
    }

    fn index_of(&mut self, id: &str) -> usize {
        if let Some(i) = self.calls.iter().position(|c| c.id == id) {
            return i;
        }
        self.calls.push(PartialToolCall {
            id: id.to_string(),
            ..Default::default()
        });
        self.calls.len() - 1
    }

    /// Apply one streamed chunk, returning the running partial for this id. A
    /// non-empty `name` overrides; `args_delta` is appended.
    pub fn push(&mut self, id: &str, name: Option<&str>, args_delta: &str) -> &PartialToolCall {
        let idx = self.index_of(id);
        let call = &mut self.calls[idx];
        if let Some(n) = name {
            if !n.is_empty() {
                call.name = Some(n.to_string());
            }
        }
        call.arguments.push_str(args_delta);
        &self.calls[idx]
    }

    /// Reconcile a finalized [`ToolCall`]: its name and arguments are
    /// authoritative and replace whatever was accumulated, marking the entry
    /// complete.
    pub fn finalize(&mut self, call: ToolCall) {
        let idx = self.index_of(&call.id);
        let entry = &mut self.calls[idx];
        entry.name = Some(call.name);
        entry.arguments = call.arguments;
        entry.complete = true;
    }

    /// The running partial for `id`, if any.
    pub fn partial(&self, id: &str) -> Option<&PartialToolCall> {
        self.calls.iter().find(|c| c.id == id)
    }

    /// Calls reconciled by [`finalize`](Self::finalize), as concrete
    /// [`ToolCall`]s, without clearing state.
    pub fn completed(&self) -> Vec<ToolCall> {
        self.calls
            .iter()
            .filter(|c| c.complete)
            .map(PartialToolCall::to_tool_call)
            .collect()
    }

    /// Drain every accumulated call as a [`ToolCall`], clearing the accumulator.
    pub fn drain_completed(&mut self) -> Vec<ToolCall> {
        let out = self.calls.iter().map(PartialToolCall::to_tool_call).collect();
        self.calls.clear();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_interleaved_calls_by_id() {
        let mut acc = ToolCallAccumulator::new();
        acc.push("a", Some("add"), "{\"x\":");
        acc.push("b", Some("mul"), "{\"y\":");
        acc.push("a", None, "1}");
        acc.push("b", None, "2}");

        assert_eq!(acc.partial("a").unwrap().name.as_deref(), Some("add"));
        assert_eq!(acc.partial("a").unwrap().arguments, "{\"x\":1}");
        assert_eq!(acc.partial("b").unwrap().arguments, "{\"y\":2}");
    }

    #[test]
    fn finalize_is_authoritative_and_marks_complete() {
        let mut acc = ToolCallAccumulator::new();
        acc.push("a", Some("add"), "{\"x\":1"); // truncated mid-stream
        assert!(acc.completed().is_empty());

        acc.finalize(ToolCall {
            id: "a".to_string(),
            name: "add".to_string(),
            arguments: "{\"x\":1,\"y\":2}".to_string(),
        });

        let done = acc.completed();
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].arguments, "{\"x\":1,\"y\":2}");
    }

    #[test]
    fn drain_empties() {
        let mut acc = ToolCallAccumulator::new();
        acc.push("a", Some("add"), "{}");
        assert_eq!(acc.drain_completed().len(), 1);
        assert!(acc.drain_completed().is_empty());
    }
}
