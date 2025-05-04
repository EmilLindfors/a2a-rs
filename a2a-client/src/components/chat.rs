use a2a_rs::{
    domain::{Message, Part, Role},
    port::client::StreamItem,
};
use futures::StreamExt;
use leptos::html::ElementChild;
use leptos::prelude::*;
use leptos::*;
use std::{cell::RefCell, rc::Rc};
use uuid::Uuid;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::client::A2AClientImpl;

#[derive(Debug, Clone)]
struct ChatMessage {
    id: String,
    content: String,
    is_user: bool,
}

#[component]
pub fn Chat() -> impl IntoView {
    // Create a client instance that will be shared
    let client = Rc::new(RefCell::new(A2AClientImpl::new(
        "ws://localhost:8081".to_string(),
    )));

    // Create signals for our chat state
    let messages = RwSignal::new(vec![]);
    let input_text = RwSignal::new("".to_string());
    let current_task_id = RwSignal::new(None);
    let is_streaming = RwSignal::new(false);

    // Function to send a message
    let send_message = move |message_text: String| {
        let client = client.clone();
        spawn_local(async move {
            // Generate a task ID
            let task_id = format!("task-{}", Uuid::new_v4());
            current_task_id.set(Some(task_id.clone()));

            // Add user message to the UI
            messages.update(|messages| {
                messages.push(ChatMessage {
                    id: Uuid::new_v4().to_string(),
                    content: message_text.clone(),
                    is_user: true,
                });
            });

            // Create A2A message
            let a2a_message = Message::user_text(message_text);

            // Start streaming
            is_streaming.set(true);

            // Create a placeholder for the assistant's response
            let assistant_msg_id = Uuid::new_v4().to_string();
            messages.update(|messages| {
                messages.push(ChatMessage {
                    id: assistant_msg_id.clone(),
                    content: "".to_string(),
                    is_user: false,
                });
            });

            // Subscribe to the task
            #[allow(clippy::await_holding_refcell_ref)]
            let client_ref = client.borrow();
            match client_ref
                .subscribe_to_task(&task_id, &a2a_message, None, None)
                .await
            {
                Ok(mut stream) => {
                    // Process streaming updates
                    #[allow(unused_assignments)]
                    let mut current_response = String::new();

                    while let Some(result) = stream.next().await {
                        match result {
                            Ok(StreamItem::StatusUpdate(update)) => {
                                if let Some(message) = &update.status.message {
                                    if message.role == Role::Agent {
                                        for part in &message.parts {
                                            if let Part::Text { text, .. } = part {
                                                // Update our accumulated response
                                                current_response = text.clone();

                                                // Update the message in our UI
                                                messages.update(|messages| {
                                                    for msg in messages.iter_mut() {
                                                        if msg.id == assistant_msg_id {
                                                            msg.content = current_response.clone();
                                                            break;
                                                        }
                                                    }
                                                });
                                            }
                                        }
                                    }
                                }

                                if update.final_ {
                                    // Final update received
                                    is_streaming.set(false);
                                    break;
                                }
                            }
                            Ok(StreamItem::ArtifactUpdate(_)) => {
                                // Handle artifact updates if needed
                            }
                            Err(e) => {
                                // Handle errors
                                web_sys::console::error_1(&wasm_bindgen::JsValue::from_str(
                                    &format!("Stream error: {}", e),
                                ));
                                is_streaming.set(false);
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    // Handle subscription errors
                    web_sys::console::error_1(&wasm_bindgen::JsValue::from_str(&format!(
                        "Subscription error: {}",
                        e
                    )));
                    is_streaming.set(false);

                    // Add error message
                    messages.update(|messages| {
                        for msg in messages.iter_mut() {
                            if msg.id == assistant_msg_id {
                                msg.content = format!("Error: {}", e);
                                break;
                            }
                        }
                    });
                }
            }
        });
    };

    // Create form handler
    let send_message_handler = move |ev: ev::SubmitEvent| {
        ev.prevent_default();
        let current_text = input_text.get();
        if !current_text.trim().is_empty() {
            send_message(current_text);
            input_text.set("".to_string());
        }
    };

    view! {
        <div class="chat-container">
            <div class="messages">
                <For
                    each=move || messages.get()
                    key=|msg| msg.id.clone()
                    let:msg
                >
                    <div class={if msg.is_user { "user-message" } else { "assistant-message" }}>
                        <p>{msg.content.clone()}</p>
                    </div>
                </For>
            </div>

            <form on:submit=send_message_handler class="input-area">
                <input
                    type="text"
                    placeholder="Type your message..."
                    value=input_text
                    on:input=move |ev| {
                        input_text.set(event_target_value(&ev));
                    }
                    disabled=is_streaming
                />
                <button
                    type="submit"
                    disabled=move || input_text.get().trim().is_empty() || is_streaming.get()
                >
                    Send
                </button>
            </form>

            {move || {
                let content = if is_streaming.get() { "Assistant is typing..." } else { "" };
                view! { <div class="streaming-indicator">{content}</div> }
            }}
        </div>
    }
}

// Helper function to get the value from an input element
fn event_target_value(ev: &ev::Event) -> String {
    let target = ev.target().unwrap();
    let target: web_sys::HtmlInputElement = target.dyn_into().unwrap();
    target.value()
}
