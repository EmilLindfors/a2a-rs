use a2a_client::client::A2AClientImpl;
use a2a_rs::{
    domain::{Message, Part, Role},
    port::client::StreamItem,
};
use futures::StreamExt;
use leptos::html::ElementChild;
use leptos::mount::mount_to_body;
use leptos::prelude::*;
use leptos::*;
use std::{cell::RefCell, rc::Rc};
use uuid::Uuid;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

fn main() {
    // Initialize console error panic hook for better error messages
    console_error_panic_hook::set_once();

    // Initialize logging to console
    _ = console_log::init_with_level(log::Level::Debug);

    // Mount the app to the body
    mount_to_body(SimpleChatApp);
}

#[component]
fn SimpleChatApp() -> impl IntoView {
    // Create a WebSocket client
    let client = Rc::new(RefCell::new(A2AClientImpl::new(
        "ws://localhost:8081".to_string(),
    )));

    // Create signals for the state
    let messages = RwSignal::new(Vec::<String>::new());
    let input = RwSignal::new(String::new());
    let is_loading = RwSignal::new(false);

    // Function to send a message
    let send_message = move |message_text: String| {
        let client = client.clone();
        spawn_local(async move {
            // Check if the input is empty
            if message_text.trim().is_empty() {
                return;
            }

            // Add the user message to the chat
            messages.update(|msgs| {
                msgs.push(format!("You: {}", message_text));
            });

            // Create an A2A message
            let message = Message::user_text(message_text);

            // Create a task ID
            let task_id = format!("task-{}", Uuid::new_v4());

            // Set loading state
            is_loading.set(true);

            // Subscribe to the task
            #[allow(clippy::await_holding_refcell_ref)]
            let client_ref = client.borrow();
            match client_ref
                .subscribe_to_task(&task_id, &message, None, None)
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
                                                messages.update(|msgs| {
                                                    // Remove the old assistant message if it exists
                                                    if msgs.len() > 0
                                                        && msgs
                                                            .last()
                                                            .unwrap()
                                                            .starts_with("Assistant:")
                                                    {
                                                        msgs.pop();
                                                    }
                                                    // Add the new message
                                                    msgs.push(format!(
                                                        "Assistant: {}",
                                                        current_response
                                                    ));
                                                });
                                            }
                                        }
                                    }
                                }

                                if update.final_ {
                                    // Final update received
                                    is_loading.set(false);
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
                                is_loading.set(false);

                                messages.update(|msgs| {
                                    msgs.push(format!("Error: {}", e));
                                });
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
                    is_loading.set(false);

                    messages.update(|msgs| {
                        msgs.push(format!("Error: {}", e));
                    });
                }
            }
        });
    };

    // Form submit handler
    let on_submit = move |ev: ev::SubmitEvent| {
        ev.prevent_default();
        let current_input = input.get();
        send_message(current_input);
        input.set("".into());
    };

    view! {
        <div class="chat-app">
            <h1>"Simple A2A Chat Example"</h1>

            <div class="messages">
                <For
                    each=move || messages.get()
                    key=|msg| msg.clone()
                    let:msg
                >
                    <div class="message">{msg}</div>
                </For>

                {move || {
                    let content = if is_loading.get() { "Assistant is typing..." } else { "" };
                    view! { <div class="typing">{content}</div> }
                }}
            </div>

            <form on:submit=on_submit>
                <input
                    type="text"
                    placeholder="Type your message..."
                    value=input
                    on:input=move |ev| {
                        input.set(event_target_value(&ev));
                    }
                    disabled=is_loading
                />
                <button
                    type="submit"
                    disabled=move || input.get().trim().is_empty() || is_loading.get()
                >"Send"</button>
            </form>
        </div>
    }
}

// Helper function to get the value from an input element
fn event_target_value(ev: &ev::Event) -> String {
    let target = ev.target().unwrap();
    let target: web_sys::HtmlInputElement = target.dyn_into().unwrap();
    target.value()
}
