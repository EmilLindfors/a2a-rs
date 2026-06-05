//! Unit tests for client-side transport negotiation.
//!
//! These drive [`TransportNegotiator`] with **fake** factories (no network) to
//! pin the ranking algorithm: client preference (registration order) dominates
//! card order, a failing `create` falls through to the next compatible
//! interface, an unknown protocol errors, and the major-version filter skips
//! incompatible interfaces.

#![cfg(feature = "client")]

use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;

use a2a_rs::domain::{
    A2AError, AgentCard, AgentInterface, ListTasksParams, ListTasksResult, Message, Task,
    TaskPushNotificationConfig,
};
use a2a_rs::{StreamEvent, Transport, TransportFactory, TransportNegotiator};

/// A no-op transport that only reports its protocol — its RPC methods are never
/// called in negotiation tests.
struct DummyTransport {
    proto: &'static str,
}

#[async_trait]
impl Transport for DummyTransport {
    fn protocol(&self) -> &str {
        self.proto
    }
    async fn send_task_message(
        &self,
        _: &str,
        _: &Message,
        _: Option<&str>,
        _: Option<u32>,
    ) -> Result<Task, A2AError> {
        unimplemented!()
    }
    async fn get_task(&self, _: &str, _: Option<u32>) -> Result<Task, A2AError> {
        unimplemented!()
    }
    async fn cancel_task(&self, _: &str) -> Result<Task, A2AError> {
        unimplemented!()
    }
    async fn set_task_push_notification(
        &self,
        _: &TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        unimplemented!()
    }
    async fn get_task_push_notification(
        &self,
        _: &str,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        unimplemented!()
    }
    async fn list_tasks(&self, _: &ListTasksParams) -> Result<ListTasksResult, A2AError> {
        unimplemented!()
    }
    async fn list_push_notification_configs(
        &self,
        _: &str,
    ) -> Result<Vec<TaskPushNotificationConfig>, A2AError> {
        unimplemented!()
    }
    async fn get_push_notification_config(
        &self,
        _: &str,
        _: &str,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        unimplemented!()
    }
    async fn delete_push_notification_config(&self, _: &str, _: &str) -> Result<(), A2AError> {
        unimplemented!()
    }
    async fn subscribe_to_task(
        &self,
        _: &str,
        _: Option<u32>,
        _: Option<&str>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent, A2AError>> + Send>>, A2AError> {
        unimplemented!()
    }
}

/// A factory that builds a [`DummyTransport`] for one protocol, optionally
/// failing `create` to exercise fall-through.
struct FakeFactory {
    proto: &'static str,
    fail: bool,
}

#[async_trait]
impl TransportFactory for FakeFactory {
    fn protocol(&self) -> &str {
        self.proto
    }
    async fn create(
        &self,
        _card: &AgentCard,
        _iface: &AgentInterface,
    ) -> Result<Box<dyn Transport>, A2AError> {
        if self.fail {
            Err(A2AError::Internal("boom".to_string()))
        } else {
            Ok(Box::new(DummyTransport { proto: self.proto }))
        }
    }
}

fn iface(proto: &str, version: &str) -> AgentInterface {
    AgentInterface {
        url: format!("http://localhost/{proto}"),
        protocol_binding: proto.to_string(),
        protocol_version: version.to_string(),
        ..Default::default()
    }
}

fn card(interfaces: Vec<AgentInterface>) -> AgentCard {
    AgentCard {
        supported_interfaces: interfaces,
        ..Default::default()
    }
}

#[tokio::test]
async fn prefers_client_order_over_card_order() {
    let negotiator = TransportNegotiator::new()
        .with(FakeFactory {
            proto: "CONNECTRPC",
            fail: false,
        })
        .with(FakeFactory {
            proto: "JSONRPC",
            fail: false,
        });
    // Card lists JSONRPC first, but the client prefers CONNECTRPC.
    let c = card(vec![iface("JSONRPC", "1.0"), iface("CONNECTRPC", "1.0")]);
    let transport = negotiator.negotiate(&c).await.unwrap();
    assert_eq!(transport.protocol(), "CONNECTRPC");
}

#[tokio::test]
async fn falls_through_on_create_failure() {
    let negotiator = TransportNegotiator::new()
        .with(FakeFactory {
            proto: "CONNECTRPC",
            fail: true,
        })
        .with(FakeFactory {
            proto: "JSONRPC",
            fail: false,
        });
    let c = card(vec![iface("CONNECTRPC", "1.0"), iface("JSONRPC", "1.0")]);
    let transport = negotiator.negotiate(&c).await.unwrap();
    assert_eq!(transport.protocol(), "JSONRPC");
}

#[tokio::test]
async fn unknown_protocol_errors() {
    let negotiator = TransportNegotiator::new().with(FakeFactory {
        proto: "JSONRPC",
        fail: false,
    });
    // `Box<dyn Transport>` isn't `Debug`, so match the Result rather than `unwrap_err`.
    let result = negotiator
        .negotiate(&card(vec![iface("GRPC", "1.0")]))
        .await;
    assert!(matches!(result, Err(A2AError::UnsupportedOperation(_))));
}

#[tokio::test]
async fn empty_interfaces_errors() {
    let negotiator = TransportNegotiator::new().with(FakeFactory {
        proto: "JSONRPC",
        fail: false,
    });
    assert!(negotiator.negotiate(&card(vec![])).await.is_err());
}

#[tokio::test]
async fn skips_incompatible_major_version() {
    let negotiator = TransportNegotiator::new().with(FakeFactory {
        proto: "JSONRPC",
        fail: false,
    });
    // v2.x is not compatible with this client.
    assert!(
        negotiator
            .negotiate(&card(vec![iface("JSONRPC", "2.0")]))
            .await
            .is_err()
    );
    // v1.x is.
    let transport = negotiator
        .negotiate(&card(vec![iface("JSONRPC", "1.5")]))
        .await
        .unwrap();
    assert_eq!(transport.protocol(), "JSONRPC");
}

#[test]
fn supported_lists_protocols_in_preference_order() {
    let negotiator = TransportNegotiator::new()
        .with(FakeFactory {
            proto: "CONNECTRPC",
            fail: false,
        })
        .with(FakeFactory {
            proto: "JSONRPC",
            fail: false,
        });
    assert_eq!(
        negotiator.supported().collect::<Vec<_>>(),
        vec!["CONNECTRPC", "JSONRPC"]
    );
}
