//! Ports (interfaces) for the A2A protocol
//!
//! Ports define the interfaces that our application needs, independent of implementation details.
//! They represent the "what" - what operations our application needs to perform.
//!
//! ## Organization
//!
//! - **Business capability ports**: Focused interfaces for specific business capabilities
//!   - `authenticator`: Authentication and authorization
//!   - `message_handler`: Message processing
//!   - `task_manager`: Task lifecycle management  
//!   - `notification_manager`: Push notifications
//!   - `streaming_handler`: Real-time updates
//!   - `event_log`: What a task's stream already said, for resuming it
//!   - `conversation_store`: Durable conversation memory for a context
//!   - `context_state`: The facts an agent keeps about a context, apart from
//!     the transcript
//!   - `request_context`: Who is calling, carried from the transport inward

// Business capability ports (focused domain interfaces)
pub mod authenticator;
pub mod client;
pub mod context_state;
pub mod conversation_store;
pub mod event_log;
pub mod interceptor;
pub mod message_handler;
pub mod notification_manager;
pub mod request_context;
pub mod retention;
pub mod streaming_handler;
pub mod task_manager;

// Re-export business capability interfaces
pub use authenticator::{
    AuthContext, AuthContextExtractor, AuthPrincipal, Authenticator, CompositeAuthenticator,
};
pub use client::{StreamEvent, StreamItem, Transport};
pub use context_state::{AsyncContextStateStore, NoContextState};
pub use conversation_store::{
    AsyncConversationStore, AsyncConversationStoreExt, NoConversationMemory,
};
pub use event_log::{AsyncEventLog, Replay};
pub use interceptor::{CallContext, CallInterceptor, CallSide, run_after, run_before};
pub use message_handler::AsyncMessageHandler;
pub use notification_manager::{
    AsyncNotificationManager, AsyncNotificationManagerExt, AsyncPushNotifier, NoopPushNotifier,
};
pub use request_context::RequestContext;
pub use retention::AsyncRetention;
pub use streaming_handler::{
    AsyncStreamingHandler, SeqEvent, Subscriber as StreamingSubscriber, UpdateEvent,
};
pub use task_manager::{
    AsyncTaskLifecycle, AsyncTaskLifecycleExt, AsyncTaskQuery, AsyncTaskVersioning,
};
