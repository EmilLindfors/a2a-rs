pub mod client;
pub mod components;

use components::Chat;
use leptos::mount::mount_to_body;
use leptos::prelude::*;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <main>
            <h1>"A2A Chat with Leptos"</h1>
            <p>"This is a simple chat interface using A2A WebSocket connection"</p>
            <Chat />
        </main>
    }
}

#[cfg(feature = "csr")]
pub fn main() {
    // Initialize console error panic hook for better error messages
    console_error_panic_hook::set_once();

    // Initialize logging to console
    _ = console_log::init_with_level(log::Level::Debug);

    // Mount the app to the body
    mount_to_body(App);
}
