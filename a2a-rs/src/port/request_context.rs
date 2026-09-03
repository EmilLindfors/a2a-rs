//! What a transport knows about an inbound call, beyond its payload.

use crate::port::authenticator::AuthPrincipal;

/// Who is calling, and which conversation the call belongs to.
///
/// Built by the transport adapter that accepted the request and passed down
/// through `TaskService` to
/// [`AsyncMessageHandler`](crate::port::AsyncMessageHandler). One value rather
/// than a widening parameter list: the context id, the principal and (later) the
/// tenant are all facts about *this* request, and a handler that wants one
/// usually wants the others.
///
/// Not to be confused with [`CallContext`](crate::port::CallContext), which is
/// interceptor metadata about the call being dispatched (method name, side of
/// the wire) and says nothing about who made it.
///
/// # The principal
///
/// [`principal`](Self::principal) is `None` when the agent serves anonymous
/// callers — no authenticator is configured, so there is nobody to name. It is
/// what [`AsyncConversationStore`](crate::port::AsyncConversationStore) compares
/// against a context's recorded owner, which is why the transport has to carry
/// it rather than the handler guessing: a handler that cannot tell two callers
/// apart hands the second one the first one's conversation.
#[derive(Debug, Clone, Default)]
pub struct RequestContext {
    /// The context id the caller supplied, if any. Empty on a first turn that
    /// lets the agent pick one.
    context_id: Option<String>,
    /// The authenticated principal, or `None` on an agent that does not
    /// authenticate.
    principal: Option<AuthPrincipal>,
}

impl RequestContext {
    /// A call from nobody in particular: no context, no principal.
    ///
    /// The right context for an internal caller that is not serving a request —
    /// a bridge invoking a handler directly, or a test.
    pub fn anonymous() -> Self {
        Self::default()
    }

    /// Name the conversation this call belongs to.
    ///
    /// An empty string is the same as no context at all, which is what a wire
    /// message with an unset `context_id` decodes to.
    #[must_use]
    pub fn with_context(mut self, context_id: impl Into<String>) -> Self {
        let context_id = context_id.into();
        self.context_id = (!context_id.is_empty()).then_some(context_id);
        self
    }

    /// Attach the principal the transport authenticated.
    #[must_use]
    pub fn with_principal(mut self, principal: impl Into<Option<AuthPrincipal>>) -> Self {
        self.principal = principal.into();
        self
    }

    /// The context id the caller supplied.
    pub fn context_id(&self) -> Option<&str> {
        self.context_id.as_deref()
    }

    /// The authenticated principal, with whatever claims the authenticator
    /// attached.
    pub fn principal(&self) -> Option<&AuthPrincipal> {
        self.principal.as_ref()
    }

    /// The authenticated principal's id — the identity a conversation is owned
    /// by.
    pub fn caller(&self) -> Option<&str> {
        self.principal.as_ref().map(|p| p.id.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_anonymous_call_names_nobody() {
        let ctx = RequestContext::anonymous();
        assert_eq!(ctx.context_id(), None);
        assert_eq!(ctx.caller(), None);
    }

    #[test]
    fn an_empty_context_id_is_no_context() {
        // What an unset wire `context_id` decodes to, so the transports do not
        // each have to remember to filter it.
        let ctx = RequestContext::anonymous().with_context("");
        assert_eq!(ctx.context_id(), None);
    }

    #[test]
    fn the_caller_is_the_principals_id() {
        let ctx = RequestContext::anonymous()
            .with_context("ctx-1")
            .with_principal(AuthPrincipal::new("alice".to_string(), "jwt".to_string()));

        assert_eq!(ctx.context_id(), Some("ctx-1"));
        assert_eq!(ctx.caller(), Some("alice"));
        assert_eq!(ctx.principal().map(|p| p.scheme.as_str()), Some("jwt"));
    }
}
