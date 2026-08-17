//! Fitting a message list into a model's context window.
//!
//! Pure: no I/O, no provider, no clock. [`fit`] decides what to send and says
//! when what is left still does not fit, and the caller — which owns the
//! conversation store and the LLM — acts on that.

use a2a_llm::{ChatMessage, MessageRole};

use super::TokenEstimate;

/// How much of a model's context window a request may use, and when to compact.
///
/// Sizes are in tokens as counted by whichever [`TokenEstimate`] is in use, so
/// they are approximate by construction. Leave headroom.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContextBudget {
    /// Hard ceiling on the request, including the system prompt and tool
    /// definitions.
    pub max_input_tokens: usize,
    /// Held back from `max_input_tokens` for the model's reply. A model that
    /// fills its window with input has nowhere to answer.
    pub reserve_for_output: usize,
    /// Fraction of the usable budget at which [`fit`] asks for compaction,
    /// before anything has to be dropped.
    pub compact_at: f32,
    /// Turns at the end of the conversation that compaction may not fold into a
    /// summary. The recent turns are the ones a model needs verbatim.
    pub keep_recent_turns: usize,
    /// Longest a single tool result may be, in characters. Tool output is the
    /// largest and most redundant thing in a transcript, so it is trimmed first
    /// and by itself.
    pub max_tool_result_chars: usize,
}

impl Default for ContextBudget {
    fn default() -> Self {
        Self {
            max_input_tokens: 100_000,
            reserve_for_output: 8_000,
            compact_at: 0.8,
            keep_recent_turns: 4,
            max_tool_result_chars: 8_000,
        }
    }
}

/// Smallest summary worth asking for, in tokens. Below this there is no room to
/// name what a later turn needs — decisions, identifiers, open threads — and
/// what comes back is a sentence *about* the conversation rather than notes
/// from it.
const MIN_SUMMARY_TOKENS: usize = 256;

impl ContextBudget {
    /// Tokens available to the request after reserving room for the reply.
    /// Saturates at zero rather than wrapping when the reserve is larger than
    /// the ceiling.
    pub fn usable(&self) -> usize {
        self.max_input_tokens
            .saturating_sub(self.reserve_for_output)
    }

    /// Ceiling on a summary standing in for `replaced` tokens of transcript.
    ///
    /// Asking for a summary with no ceiling lets a verbose model return one
    /// about as long as what it replaces, and the digest is re-sent on every
    /// later turn — so the cost lands again each turn while the saving is zero.
    /// A tenth of the usable window, never more than half of what it replaces,
    /// and never below [`MIN_SUMMARY_TOKENS`]. The last floor is what covers
    /// `max_input_tokens = 0` ("no ceiling"), where a tenth of `usable` is a
    /// meaningless number.
    pub fn summary_tokens(&self, replaced: usize) -> usize {
        (self.usable() / 10)
            .min(replaced / 2)
            .max(MIN_SUMMARY_TOKENS)
    }

    /// The size at which compaction is worth doing, below [`Self::usable`].
    fn compaction_threshold(&self) -> usize {
        let fraction = self.compact_at.clamp(0.0, 1.0) as f64;
        (self.usable() as f64 * fraction) as usize
    }
}

/// What [`fit`] did, and what the caller still has to do about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// Everything fits with room to spare. Send it.
    AsIs,
    /// Over the compaction threshold but still under the ceiling. Sending this
    /// works; summarizing before the next turn keeps it working.
    ShouldCompact,
    /// Something was dropped to make it fit. Sending this works and the model
    /// has less to go on than the caller gave it.
    Trimmed,
    /// Still over the ceiling with nothing left that trimming may drop. Sending
    /// this may be refused by the provider. Compaction is the only thing that
    /// helps and it helps the *next* turn, since the digest is written after
    /// this request was built.
    OverBudget,
}

/// Why a message list cannot be made to fit.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContextError {
    /// The system prompt and the current user message alone exceed the budget.
    ///
    /// This is refused rather than truncated: silently dropping half of the
    /// question produces a confident answer to something the caller did not
    /// ask. The message names both numbers and the setting that changes them.
    #[error(
        "the system prompt and this message need about {needed} tokens, over the {usable} available \
         ({max_input_tokens} max_input_tokens less {reserve_for_output} reserve_for_output) — \
         raise `[llm.context] max_input_tokens` or shorten the system prompt"
    )]
    Irreducible {
        /// Estimated tokens of the parts that cannot be dropped.
        needed: usize,
        /// Tokens available after the output reserve.
        usable: usize,
        /// The configured ceiling, repeated so the message stands alone.
        max_input_tokens: usize,
        /// The configured reserve, likewise.
        reserve_for_output: usize,
    },
}

/// Marks where a tool result was cut. Distinctive so it is obvious in a
/// transcript that the model saw less than the tool returned.
const ELISION: &str = "\n… [{n} characters elided] …\n";

/// Trim one tool result to `max_chars`, keeping the head and the tail.
///
/// Both ends carry signal: the head has the shape of the result (a header row,
/// an opening brace, an error class) and the tail has the summary or the last
/// records. Cutting only the tail loses the second, and cutting only the head
/// loses the first.
pub fn cap_tool_result(content: &str, max_chars: usize) -> Option<String> {
    if content.chars().count() <= max_chars {
        return None;
    }
    // Below this there is no room for both ends plus the marker, so keep the
    // head only rather than emitting a message that is mostly elision notice.
    let marker_len = ELISION.len() + 8;
    if max_chars <= marker_len {
        let head: String = content.chars().take(max_chars).collect();
        return Some(head);
    }

    let keep = max_chars - marker_len;
    let head_len = keep * 2 / 3;
    let tail_len = keep - head_len;
    let total = content.chars().count();

    let head: String = content.chars().take(head_len).collect();
    let tail: String = content.chars().skip(total - tail_len).collect();
    let elided = total - head_len - tail_len;
    Some(format!(
        "{head}{}{tail}",
        ELISION.replace("{n}", &elided.to_string())
    ))
}

/// Trim `messages` until they fit `budget`, cheapest loss first.
///
/// The order is deliberate. Capping tool results costs the least and usually
/// suffices, because one runaway tool response is the common way a transcript
/// blows past a window. Dropping whole tool call/result pairs costs the
/// evidence but keeps the assistant text that concluded from it. Only then is
/// there nothing left but the turns themselves, which is compaction's job and
/// not this function's.
///
/// The system prompt and the trailing user message are never touched. If those
/// alone exceed the budget this returns [`ContextError::Irreducible`].
pub fn fit(
    messages: &mut Vec<ChatMessage>,
    budget: &ContextBudget,
    estimator: &dyn TokenEstimate,
) -> Result<Fit, ContextError> {
    let usable = budget.usable();

    // The floor: what has to survive whatever else goes. Checked first so an
    // impossible budget is reported as such rather than after a pointless
    // trimming pass.
    let floor: Vec<&ChatMessage> = messages
        .iter()
        .enumerate()
        .filter(|(index, message)| {
            matches!(message.role, MessageRole::System) || *index == messages.len() - 1
        })
        .map(|(_, message)| message)
        .collect();
    let floor_tokens = estimator.estimate_messages(&floor);
    if floor_tokens > usable {
        return Err(ContextError::Irreducible {
            needed: floor_tokens,
            usable,
            max_input_tokens: budget.max_input_tokens,
            reserve_for_output: budget.reserve_for_output,
        });
    }

    let mut trimmed = false;

    // 1. Cap tool results. Cheapest, and the usual cause.
    for message in messages.iter_mut() {
        if !matches!(message.role, MessageRole::Tool) {
            continue;
        }
        if let Some(content) = message.content.as_ref()
            && let Some(capped) = cap_tool_result(content, budget.max_tool_result_chars)
        {
            message.content = Some(capped);
            trimmed = true;
        }
    }

    if estimate_all(estimator, messages) <= usable {
        return Ok(verdict(trimmed, estimate_all(estimator, messages), budget));
    }

    // 2. Drop whole tool call/result pairs, oldest first, keeping the assistant
    //    text that followed them. The conclusion survives; the evidence does not.
    while estimate_all(estimator, messages) > usable {
        let Some(index) = oldest_tool_round(messages) else {
            break;
        };
        drop_tool_round(messages, index);
        trimmed = true;
    }

    if estimate_all(estimator, messages) > usable {
        // Nothing left that this function is allowed to drop. Reported apart
        // from `ShouldCompact` because the two ask different things of the
        // caller: one is a request that fits and wants summarizing before the
        // next one, this is a request that does not fit.
        return Ok(Fit::OverBudget);
    }

    Ok(verdict(trimmed, estimate_all(estimator, messages), budget))
}

fn verdict(trimmed: bool, total: usize, budget: &ContextBudget) -> Fit {
    if trimmed {
        Fit::Trimmed
    } else if total > budget.compaction_threshold() {
        Fit::ShouldCompact
    } else {
        Fit::AsIs
    }
}

fn estimate_all(estimator: &dyn TokenEstimate, messages: &[ChatMessage]) -> usize {
    let refs: Vec<&ChatMessage> = messages.iter().collect();
    estimator.estimate_messages(&refs)
}

/// Index of the oldest assistant message that requested tool calls, skipping
/// anything in the trailing message. Returns `None` when no tool round is left.
fn oldest_tool_round(messages: &[ChatMessage]) -> Option<usize> {
    let last = messages.len().saturating_sub(1);
    messages.iter().enumerate().position(|(index, message)| {
        index != last
            && matches!(message.role, MessageRole::Assistant)
            && message
                .tool_calls
                .as_ref()
                .is_some_and(|calls| !calls.is_empty())
    })
}

/// Strip the tool calls off the assistant message at `index` and remove the
/// tool results that answered them.
///
/// The assistant message is kept when it also said something, because that text
/// is the model's own summary of what the tools told it. It is removed when it
/// was nothing but a tool call, since an assistant turn with neither content nor
/// tool calls is not a valid message on any provider.
fn drop_tool_round(messages: &mut Vec<ChatMessage>, index: usize) {
    let call_ids: Vec<String> = messages[index]
        .tool_calls
        .take()
        .unwrap_or_default()
        .into_iter()
        .map(|call| call.id)
        .collect();

    messages.retain(|message| {
        !matches!(message.role, MessageRole::Tool)
            || message
                .tool_call_id
                .as_ref()
                .is_none_or(|id| !call_ids.contains(id))
    });

    // `index` still points at the same message: `retain` only removed tool
    // results, which always follow the assistant turn that requested them.
    if messages[index]
        .content
        .as_deref()
        .unwrap_or_default()
        .is_empty()
    {
        messages.remove(index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::CharEstimate;
    use a2a_llm::ToolCall;

    fn estimator() -> CharEstimate {
        CharEstimate::default()
    }

    fn budget(max_input_tokens: usize) -> ContextBudget {
        ContextBudget {
            max_input_tokens,
            reserve_for_output: 0,
            compact_at: 0.8,
            keep_recent_turns: 2,
            max_tool_result_chars: 100,
        }
    }

    fn assistant_calling(id: &str, said: &str) -> ChatMessage {
        ChatMessage {
            role: MessageRole::Assistant,
            content: (!said.is_empty()).then(|| said.to_string()),
            tool_calls: Some(vec![ToolCall {
                id: id.to_string(),
                name: "look_it_up".to_string(),
                arguments: "{}".to_string(),
            }]),
            tool_call_id: None,
            name: None,
        }
    }

    /// A tool result keeps both ends: the head carries the result's shape and
    /// the tail carries whatever it concluded with.
    #[test]
    fn a_capped_tool_result_keeps_the_head_and_the_tail() {
        let content = format!("HEAD{}TAIL", "x".repeat(5_000));
        let capped = cap_tool_result(&content, 200).expect("over the cap");

        assert!(capped.starts_with("HEAD"), "{capped}");
        assert!(capped.ends_with("TAIL"), "{capped}");
        assert!(capped.contains("elided"), "{capped}");
        assert!(capped.chars().count() < content.chars().count());
    }

    #[test]
    fn a_short_tool_result_is_left_alone() {
        assert_eq!(cap_tool_result("small", 100), None);
    }

    /// Character counts, not byte counts: slicing a multi-byte character in half
    /// would panic.
    #[test]
    fn capping_does_not_split_a_multibyte_character() {
        let content = "æ".repeat(1_000);
        let capped = cap_tool_result(&content, 120).expect("over the cap");
        assert!(capped.contains("elided"), "{capped}");
    }

    /// The usual shape of the problem: one runaway tool response, and capping it
    /// alone is enough.
    #[test]
    fn capping_tool_results_is_tried_before_anything_is_dropped() {
        let mut messages = vec![
            ChatMessage::system("be helpful"),
            ChatMessage::user("what is it"),
            assistant_calling("call-1", ""),
            ChatMessage::tool_result("call-1", "look_it_up", "y".repeat(40_000)),
            ChatMessage::user("and now"),
        ];

        let fit = fit(&mut messages, &budget(4_000), &estimator()).expect("fits after capping");

        assert_eq!(fit, Fit::Trimmed);
        assert_eq!(messages.len(), 5, "nothing should have been dropped");
        assert!(messages[3].content.as_ref().unwrap().contains("elided"));
    }

    /// When capping is not enough, whole tool rounds go — and the assistant text
    /// that concluded from them stays, because that is the part worth keeping.
    #[test]
    fn dropping_a_tool_round_keeps_what_the_assistant_said() {
        let mut messages = vec![
            ChatMessage::system("be helpful"),
            ChatMessage::user("what is it"),
            assistant_calling("call-1", "Bergen is on the west coast."),
            ChatMessage::tool_result("call-1", "look_it_up", "z".repeat(4_000)),
            ChatMessage::user("and now"),
        ];

        let fit = fit(&mut messages, &budget(60), &estimator()).expect("fits after dropping");

        assert_eq!(fit, Fit::Trimmed);
        assert!(
            messages
                .iter()
                .all(|m| !matches!(m.role, MessageRole::Tool)),
            "the tool result should be gone"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.content.as_deref() == Some("Bergen is on the west coast.")),
            "the assistant's conclusion must survive: {messages:?}"
        );
    }

    /// An assistant turn that was nothing but a tool call has nothing left to
    /// say once the call is dropped, and an assistant message with neither
    /// content nor tool calls is rejected by every provider.
    #[test]
    fn dropping_a_silent_tool_round_removes_the_assistant_turn_too() {
        let mut messages = vec![
            ChatMessage::system("be helpful"),
            ChatMessage::user("what is it"),
            assistant_calling("call-1", ""),
            ChatMessage::tool_result("call-1", "look_it_up", "z".repeat(4_000)),
            ChatMessage::user("and now"),
        ];

        fit(&mut messages, &budget(60), &estimator()).expect("fits after dropping");

        assert!(
            messages
                .iter()
                .all(|m| !matches!(m.role, MessageRole::Assistant)),
            "an empty assistant turn must not survive: {messages:?}"
        );
    }

    /// Truncating the question produces a confident answer to something nobody
    /// asked, so this is refused instead — and the error names the knob.
    #[test]
    fn a_budget_too_small_for_the_question_fails_rather_than_truncating_it() {
        let mut messages = vec![
            ChatMessage::system("be helpful"),
            ChatMessage::user("x".repeat(10_000)),
        ];

        let error = fit(&mut messages, &budget(50), &estimator()).expect_err("cannot fit");

        assert!(matches!(error, ContextError::Irreducible { .. }));
        assert!(error.to_string().contains("max_input_tokens"), "{error}");
    }

    #[test]
    fn a_conversation_well_under_the_budget_is_sent_as_is() {
        let mut messages = vec![ChatMessage::system("be helpful"), ChatMessage::user("hei")];

        assert_eq!(
            fit(&mut messages, &budget(10_000), &estimator()).unwrap(),
            Fit::AsIs
        );
    }

    /// Crossing the threshold without dropping anything is the signal to
    /// summarize before the next turn, not a report that something was lost.
    #[test]
    fn crossing_the_threshold_asks_for_compaction_without_trimming() {
        let mut messages = vec![
            ChatMessage::system("be helpful"),
            ChatMessage::assistant("a".repeat(3_600)),
            ChatMessage::user("hei"),
        ];

        assert_eq!(
            fit(&mut messages, &budget(1_100), &estimator()).unwrap(),
            Fit::ShouldCompact
        );
        assert_eq!(messages.len(), 3, "nothing should have been dropped");
    }

    /// Over the ceiling with no tool rounds left to drop. Saying `Trimmed` here
    /// would claim the request now fits, and `ShouldCompact` would claim it fits
    /// today and wants summarizing tomorrow.
    #[test]
    fn plain_turns_over_the_ceiling_report_over_budget() {
        let mut messages = vec![
            ChatMessage::system("be helpful"),
            ChatMessage::assistant("a".repeat(8_000)),
            ChatMessage::user("hei"),
        ];

        assert_eq!(
            fit(&mut messages, &budget(100), &estimator()).unwrap(),
            Fit::OverBudget
        );
    }

    /// The distinction the split exists for: the same conversation is
    /// `ShouldCompact` under a budget it fits and `OverBudget` under one it does
    /// not, and only the second is a request the provider may refuse.
    #[test]
    fn over_the_threshold_and_over_the_ceiling_are_different_answers() {
        let messages = || {
            vec![
                ChatMessage::system("be helpful"),
                ChatMessage::assistant("a".repeat(3_600)),
                ChatMessage::user("hei"),
            ]
        };

        assert_eq!(
            fit(&mut messages(), &budget(1_100), &estimator()).unwrap(),
            Fit::ShouldCompact
        );
        assert_eq!(
            fit(&mut messages(), &budget(500), &estimator()).unwrap(),
            Fit::OverBudget
        );
    }

    /// The cap is what stops a "summary" that is as long as the transcript, so
    /// it has to stay well under what is being replaced.
    #[test]
    fn a_summary_is_capped_below_what_it_replaces() {
        let budget = ContextBudget::default();
        let replaced = 40_000;
        let cap = budget.summary_tokens(replaced);

        assert!(cap < replaced / 2, "{cap}");
        assert!(cap >= MIN_SUMMARY_TOKENS, "{cap}");
    }

    /// `max_input_tokens = 0` means no ceiling, which makes a fraction of the
    /// usable window meaningless — what is being replaced is the only number
    /// left to size against.
    #[test]
    fn an_uncapped_budget_sizes_the_summary_against_the_transcript() {
        let budget = ContextBudget {
            max_input_tokens: usize::MAX,
            ..ContextBudget::default()
        };
        assert_eq!(budget.summary_tokens(10_000), 5_000);
    }

    /// A short conversation cannot be squeezed into a few tokens, and asking
    /// for that yields a sentence about the conversation instead of notes from
    /// it. Whether the result is worth keeping is then decided by measuring it.
    #[test]
    fn a_small_transcript_still_gets_room_to_write() {
        assert_eq!(
            ContextBudget::default().summary_tokens(10),
            MIN_SUMMARY_TOKENS
        );
    }

    #[test]
    fn a_reserve_larger_than_the_ceiling_does_not_wrap() {
        let budget = ContextBudget {
            max_input_tokens: 100,
            reserve_for_output: 500,
            ..ContextBudget::default()
        };
        assert_eq!(budget.usable(), 0);
    }
}
