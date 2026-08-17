//! The two tools that let a model write to the state bag.
//!
//! Bound to one turn: the source is built with the context and the caller of
//! the request it serves, so a tool call cannot land in another conversation.
//! That is also why it is not one of the handler's long-lived [`ToolSource`]s —
//! those are shared across every turn and know nothing about who is asking.

use std::sync::Arc;

use a2a_llm::{ToolCall, ToolDefinition};
use a2a_rs::domain::{A2AError, ContextId, ContextState, StateKey, StateScope};
use a2a_rs::port::AsyncContextStateStore;
use async_trait::async_trait;

use super::tools::ToolSource;

/// What the model calls to write a value.
pub const REMEMBER_TOOL: &str = "remember";
/// What the model calls to drop one.
pub const FORGET_TOOL: &str = "forget";

/// How the pair is named in a report about tool names.
pub const MEMORY_SOURCE_LABEL: &str = "the built-in memory tools";

/// Whether `name` is one of the two built-in memory tools.
///
/// Only meaningful when `remember = true`: with the bag off these are ordinary
/// names an MCP server may use, and the source that owns them is not this one.
pub fn is_memory_tool(name: &str) -> bool {
    name == REMEMBER_TOOL || name == FORGET_TOOL
}

/// How the state bag is introduced in the prompt.
///
/// Says what the keys mean and how to change them, because a model that cannot
/// tell a remembered fact from a system instruction will neither trust it nor
/// update it.
const STATE_PREAMBLE: &str = "\
What you have been asked to remember. A key beginning `user:` belongs to the \
person you are talking to and is here in every conversation with them; a key \
with no prefix belongs to this conversation only. Call `remember` to add or \
replace one and `forget` to drop one.";

/// Marks a block cut short by `max_state_chars`.
const ELISION: &str = "\n(… more was remembered than fits here)";

/// Render the state bag for the prompt, or `None` when nothing is remembered.
///
/// `max_chars` is a backstop. The cap is enforced where a value is written,
/// because a system message is never trimmed by `fit` — but a bag written under
/// a larger ceiling than the one now configured has to be cut somewhere, and
/// silently sending it would put the request over a budget that says it fits.
pub fn render_state(state: &ContextState, max_chars: usize) -> Option<String> {
    if state.is_empty() {
        return None;
    }

    let mut rendered = String::from(STATE_PREAMBLE);
    let mut elided = false;
    for (key, value) in state.iter() {
        let entry = format!("\n{key} = {value}");
        if rendered.chars().count() + entry.chars().count() > max_chars {
            elided = true;
            continue;
        }
        rendered.push_str(&entry);
    }

    // Nothing fit, so there is nothing to say but the preamble — which on its
    // own reads as "you remember nothing" and is worse than sending no block.
    if rendered.len() == STATE_PREAMBLE.len() {
        tracing::warn!(
            max_state_chars = max_chars,
            "nothing in the state bag fits `[handler.llm.context] max_state_chars`"
        );
        return None;
    }
    if elided {
        rendered.push_str(ELISION);
    }
    Some(rendered)
}

/// `remember` and `forget`, over the state bag of one context.
pub struct MemoryToolSource {
    store: Arc<dyn AsyncContextStateStore>,
    context_id: ContextId,
    /// The principal this turn authenticated, which is both the owner check and
    /// the storage key for `user:` values.
    caller: Option<String>,
    max_state_chars: usize,
}

impl MemoryToolSource {
    pub fn new(
        store: Arc<dyn AsyncContextStateStore>,
        context_id: ContextId,
        caller: Option<String>,
        max_state_chars: usize,
    ) -> Self {
        Self {
            store,
            context_id,
            caller,
            max_state_chars,
        }
    }

    fn caller(&self) -> Option<&str> {
        self.caller.as_deref()
    }

    /// Write one value, or say why not.
    ///
    /// Every refusal here comes back as tool output rather than as an error: a
    /// key it may not use and a bag that is full are both things the model can
    /// act on — pick another key, drop something first — where a failed task is
    /// not.
    async fn remember(&self, key: &str, value: &str) -> String {
        let key: StateKey = match key.parse() {
            Ok(key) => key,
            Err(e) => return format!("not remembered: {e}"),
        };

        if key.scope() == StateScope::Temp {
            return format!(
                "'{key}' is scoped to this turn, so it was not stored — drop the `temp:` prefix \
                 to keep it"
            );
        }

        if let Some(refusal) = self.room_for(&key, value).await {
            return refusal;
        }

        match self
            .store
            .remember(&self.context_id, self.caller(), &key, value)
            .await
        {
            Ok(()) => match key.scope() {
                StateScope::User => format!("remembered '{key}' for this user"),
                _ => format!("remembered '{key}' for this conversation"),
            },
            // A principal-less `user:` key lands here, and its message names the
            // fix. Anything else is the store being unreachable, which is news
            // the model can report rather than work around.
            Err(e) => format!("not remembered: {e}"),
        }
    }

    /// Whether the bag has room for this write, or the refusal to send back.
    ///
    /// Measured against what the block would render as, not against the value
    /// alone, since that is what actually reaches the request. Replacing an
    /// existing key is measured with the old value gone.
    async fn room_for(&self, key: &StateKey, value: &str) -> Option<String> {
        let mut projected = match self.store.load_state(&self.context_id, self.caller()).await {
            Ok(state) => state,
            // The size check needs the current bag. Without it, letting the
            // write through risks one over-budget request; refusing it loses
            // the fact outright.
            Err(e) => {
                tracing::warn!("could not size the state bag before a write: {e}");
                return None;
            }
        };
        projected.insert(key.clone(), value);

        let rendered = render_state(&projected, usize::MAX)
            .map(|block| block.chars().count())
            .unwrap_or(0);
        if rendered > self.max_state_chars {
            return Some(format!(
                "not remembered: this agent keeps at most {} characters of memory and '{key}' \
                 would take it to {rendered} — `forget` something first, or store less",
                self.max_state_chars
            ));
        }
        None
    }

    async fn forget(&self, key: &str) -> String {
        let key: StateKey = match key.parse() {
            Ok(key) => key,
            Err(e) => return format!("not forgotten: {e}"),
        };

        match self
            .store
            .forget(&self.context_id, self.caller(), &key)
            .await
        {
            Ok(true) => format!("forgot '{key}'"),
            Ok(false) => format!("'{key}' was not remembered"),
            Err(e) => format!("not forgotten: {e}"),
        }
    }
}

#[async_trait]
impl ToolSource for MemoryToolSource {
    fn tool_defs(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: REMEMBER_TOOL.to_string(),
                description: "Remember a fact for later turns. Prefix the key with `user:` for \
                              something true of the person regardless of what is being discussed \
                              (their name, a preference); use no prefix for something true only \
                              of this conversation. Writing a key that already exists replaces \
                              it. Keep values short."
                    .to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "key": {
                            "type": "string",
                            "description": "What the value is called, e.g. `user:preferred_units` or `deadline`."
                        },
                        "value": {
                            "type": "string",
                            "description": "The fact to remember, as short as it can be and still be useful."
                        }
                    },
                    "required": ["key", "value"]
                }),
            },
            ToolDefinition {
                name: FORGET_TOOL.to_string(),
                description: "Drop a remembered value once it is wrong or no longer needed. \
                              Takes the key exactly as it appears in what you remember, `user:` \
                              prefix included."
                    .to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "key": {
                            "type": "string",
                            "description": "The key to drop, spelled as it appears in what you remember."
                        }
                    },
                    "required": ["key"]
                }),
            },
        ]
    }

    fn has_tool(&self, name: &str) -> bool {
        is_memory_tool(name)
    }

    fn label(&self) -> String {
        MEMORY_SOURCE_LABEL.to_string()
    }

    async fn invoke(&self, _task_id: &str, call: &ToolCall) -> Result<String, A2AError> {
        let args: serde_json::Value = serde_json::from_str(&call.arguments)
            .map_err(|e| A2AError::InvalidParams(format!("tool arguments must be JSON: {e}")))?;
        let key = args.get("key").and_then(|k| k.as_str()).unwrap_or_default();

        Ok(match call.name.as_str() {
            REMEMBER_TOOL => {
                // A value of any JSON type, rendered as text: a model that
                // answers with a number rather than a string is right about the
                // fact and wrong about the schema, and refusing it teaches it
                // nothing.
                let value = match args.get("value") {
                    Some(serde_json::Value::String(text)) => text.clone(),
                    Some(other) => other.to_string(),
                    None => String::new(),
                };
                if value.trim().is_empty() {
                    format!("not remembered: '{key}' was given no value — use `forget` to drop it")
                } else {
                    self.remember(key, &value).await
                }
            }
            FORGET_TOOL => self.forget(key).await,
            other => {
                return Err(A2AError::Internal(format!(
                    "memory source called with unknown tool '{other}'"
                )));
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a2a_rs::InMemoryTaskStorage;
    use std::str::FromStr;

    fn source(caller: Option<&str>, max_state_chars: usize) -> MemoryToolSource {
        MemoryToolSource::new(
            Arc::new(InMemoryTaskStorage::new()),
            ContextId::from_str("ctx-1").unwrap(),
            caller.map(str::to_string),
            max_state_chars,
        )
    }

    fn call(name: &str, arguments: serde_json::Value) -> ToolCall {
        ToolCall {
            id: "call-1".to_string(),
            name: name.to_string(),
            arguments: arguments.to_string(),
        }
    }

    async fn state_of(source: &MemoryToolSource) -> ContextState {
        source
            .store
            .load_state(&source.context_id, source.caller())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_remembered_value_comes_back_on_the_next_load() {
        let source = source(Some("alice"), 2_000);
        let said = source
            .invoke(
                "task-1",
                &call(
                    REMEMBER_TOOL,
                    serde_json::json!({"key": "project", "value": "a2a-rs"}),
                ),
            )
            .await
            .unwrap();
        assert!(said.contains("remembered 'project'"), "{said}");

        let state = state_of(&source).await;
        assert_eq!(state.get(&"project".parse().unwrap()), Some("a2a-rs"));
    }

    #[tokio::test]
    async fn forgetting_says_whether_anything_was_there() {
        let source = source(Some("alice"), 2_000);
        source
            .invoke(
                "task-1",
                &call(
                    REMEMBER_TOOL,
                    serde_json::json!({"key": "project", "value": "a2a-rs"}),
                ),
            )
            .await
            .unwrap();

        let said = source
            .invoke(
                "task-1",
                &call(FORGET_TOOL, serde_json::json!({"key": "project"})),
            )
            .await
            .unwrap();
        assert!(said.contains("forgot 'project'"), "{said}");

        let said = source
            .invoke(
                "task-1",
                &call(FORGET_TOOL, serde_json::json!({"key": "project"})),
            )
            .await
            .unwrap();
        assert!(said.contains("was not remembered"), "{said}");
    }

    /// The scope exists to stop `temp:x` becoming an ordinary key that outlives
    /// the turn under a name saying it does not.
    #[tokio::test]
    async fn a_temp_key_is_not_stored_and_says_so() {
        let source = source(Some("alice"), 2_000);
        let said = source
            .invoke(
                "task-1",
                &call(
                    REMEMBER_TOOL,
                    serde_json::json!({"key": "temp:draft", "value": "x"}),
                ),
            )
            .await
            .unwrap();
        assert!(said.contains("scoped to this turn"), "{said}");
        assert!(state_of(&source).await.is_empty());
    }

    /// With no authenticator there is no principal to file a `user:` key under,
    /// and filing it against the context would promise a lifetime it lacks.
    #[tokio::test]
    async fn a_user_key_without_a_principal_is_refused_with_the_fix() {
        let source = source(None, 2_000);
        let said = source
            .invoke(
                "task-1",
                &call(
                    REMEMBER_TOOL,
                    serde_json::json!({"key": "user:tone", "value": "brief"}),
                ),
            )
            .await
            .unwrap();
        assert!(said.contains("not remembered"), "{said}");
        assert!(said.contains("server.auth"), "{said}");
        assert!(state_of(&source).await.is_empty());
    }

    #[tokio::test]
    async fn an_unknown_scope_prefix_is_refused_rather_than_stored_verbatim() {
        let source = source(Some("alice"), 2_000);
        let said = source
            .invoke(
                "task-1",
                &call(
                    REMEMBER_TOOL,
                    serde_json::json!({"key": "app:tone", "value": "brief"}),
                ),
            )
            .await
            .unwrap();
        assert!(said.contains("not a memory scope"), "{said}");
        assert!(state_of(&source).await.is_empty());
    }

    /// The block is a system message and `fit` never trims one, so the ceiling
    /// has to hold at the write.
    #[tokio::test]
    async fn a_write_past_the_ceiling_is_refused_and_names_the_way_out() {
        let source = source(Some("alice"), STATE_PREAMBLE.len() + 40);
        let said = source
            .invoke(
                "task-1",
                &call(
                    REMEMBER_TOOL,
                    serde_json::json!({"key": "essay", "value": "x".repeat(200)}),
                ),
            )
            .await
            .unwrap();
        assert!(said.contains("not remembered"), "{said}");
        assert!(said.contains("forget"), "{said}");
        assert!(state_of(&source).await.is_empty());
    }

    /// Replacing a key must be measured with the old value gone, or a bag near
    /// its ceiling could never be corrected.
    #[tokio::test]
    async fn replacing_a_value_is_sized_without_the_one_it_replaces() {
        let source = source(Some("alice"), STATE_PREAMBLE.len() + 120);
        let long = "x".repeat(100);
        for _ in 0..3 {
            let said = source
                .invoke(
                    "task-1",
                    &call(
                        REMEMBER_TOOL,
                        serde_json::json!({"key": "note", "value": long}),
                    ),
                )
                .await
                .unwrap();
            assert!(said.contains("remembered 'note'"), "{said}");
        }
        assert_eq!(state_of(&source).await.len(), 1);
    }

    #[tokio::test]
    async fn a_value_with_no_content_is_refused_and_points_at_forget() {
        let source = source(Some("alice"), 2_000);
        let said = source
            .invoke(
                "task-1",
                &call(
                    REMEMBER_TOOL,
                    serde_json::json!({"key": "project", "value": "  "}),
                ),
            )
            .await
            .unwrap();
        assert!(said.contains("forget"), "{said}");
        assert!(state_of(&source).await.is_empty());
    }

    /// A model that answers with a number is right about the fact and wrong
    /// about the schema.
    #[tokio::test]
    async fn a_non_string_value_is_rendered_rather_than_refused() {
        let source = source(Some("alice"), 2_000);
        source
            .invoke(
                "task-1",
                &call(
                    REMEMBER_TOOL,
                    serde_json::json!({"key": "count", "value": 3}),
                ),
            )
            .await
            .unwrap();
        assert_eq!(
            state_of(&source).await.get(&"count".parse().unwrap()),
            Some("3")
        );
    }

    #[test]
    fn an_empty_bag_renders_no_block() {
        assert_eq!(render_state(&ContextState::new(), 2_000), None);
    }

    #[test]
    fn the_block_names_the_keys_and_how_to_change_them() {
        let mut state = ContextState::new();
        state.insert("user:tone".parse().unwrap(), "brief");
        state.insert("project".parse().unwrap(), "a2a-rs");

        let block = render_state(&state, 2_000).unwrap();
        assert!(block.contains("user:tone = brief"), "{block}");
        assert!(block.contains("project = a2a-rs"), "{block}");
        assert!(block.contains("remember"), "{block}");
        assert!(block.contains("forget"), "{block}");
    }

    /// A bag written under a larger ceiling than the one now configured still
    /// has to produce a request that fits.
    #[test]
    fn a_block_over_the_ceiling_is_cut_and_says_so() {
        let mut state = ContextState::new();
        state.insert("short".parse().unwrap(), "ok");
        state.insert("long".parse().unwrap(), "x".repeat(500));

        let block = render_state(&state, STATE_PREAMBLE.len() + 20).unwrap();
        assert!(block.contains("short = ok"), "{block}");
        assert!(!block.contains("xxxxx"), "{block}");
        assert!(block.contains("more was remembered"), "{block}");
    }
}
