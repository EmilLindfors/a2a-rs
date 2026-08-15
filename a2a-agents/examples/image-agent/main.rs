//! An agent whose handler is Rust, shipped as its own container image.
//!
//! The declarative path (`[handler] type = "llm"`, MCP tools, delegation) covers
//! a lot, and then some agent needs code: a rate table, a domain calculation, a
//! library call. This example is that agent — a shipping-quote handler no TOML
//! can express — packaged so the platform runs it like any other:
//!
//! 1. The config (`agent.toml`) carries `[runtime] image`, naming this image.
//! 2. `ContainerRuntime` bind-mounts the config at `/etc/agent.toml`, points
//!    `A2A_CONFIG` at it, sets `HOST=0.0.0.0`, publishes the port — and starts
//!    the image with **no command override**, so the `ENTRYPOINT` below runs.
//! 3. Everything downstream is unchanged: health probes read the agent card,
//!    `a2a ps` lists it, peers resolve it by skill.
//!
//! `README.md` in this directory is the walkthrough. Run it straight from the
//! repo with `cargo run -p a2a-agents --example image-agent`.

use std::env;

use a2a_agents::core::{AgentBuilder, BuildError};
use a2a_rs::{
    domain::{A2AError, Message, Part, Role, Task, TaskState, TaskStatus},
    port::{AsyncMessageHandler, RequestContext},
};
use async_trait::async_trait;
use uuid::Uuid;

/// Where the platform mounts an agent's config, and the variable naming it.
///
/// Reading the variable rather than a hard-coded path is the one thing an image
/// has to do to be deployable: the path is the runtime's to choose, and a
/// deployed config lands wherever it says.
const CONFIG_ENV: &str = "A2A_CONFIG";

/// The config to read outside a container, so `cargo run --example` works from
/// the repo root without setting anything.
const LOCAL_CONFIG: &str = "a2a-agents/examples/image-agent/agent.toml";

/// Shipping zones and what they cost: a fixed handling fee plus a rate per kilo.
///
/// A table like this is the everyday reason to reach for a custom handler. It is
/// not a prompt, and it must not be one — the answer has to be the same every
/// time, and be wrong in a way someone can fix.
const RATES: [(&str, f64, f64); 4] = [
    ("domestic", 4.50, 1.20),
    ("eu", 9.00, 2.40),
    ("us", 14.00, 3.75),
    ("row", 19.50, 5.10),
];

/// Weight ceiling, in kilograms. Past this it is freight, and freight is quoted
/// by a human.
const MAX_KG: f64 = 30.0;

/// One word as a weight in kilograms: `2.5`, `2.5kg`, or `800g`.
///
/// A bare number is kilograms — the unit people give a parcel weight in — and
/// grams are converted rather than rejected, since half the ways to say "under a
/// kilo" use them.
fn weight_kg(word: &str) -> Option<f64> {
    if let Some(kg) = word.strip_suffix("kg") {
        return kg.parse().ok();
    }
    if let Some(grams) = word.strip_suffix('g') {
        return grams.parse::<f64>().ok().map(|grams| grams / 1000.0);
    }
    word.parse().ok()
}

/// A parsed request: what to quote for.
struct Shipment {
    kg: f64,
    zone: &'static str,
}

impl Shipment {
    /// Read `2.5 kg to eu` out of whatever the user typed, in any order and with
    /// any of the noise words around it.
    ///
    /// Deliberately forgiving about shape and strict about values: a message
    /// naming no weight is a question to ask back, while 40 kg is an answer this
    /// agent must not invent.
    fn parse(text: &str) -> Result<Self, String> {
        let lower = text.to_lowercase();
        let words: Vec<&str> = lower
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '.')
            .filter(|w| !w.is_empty())
            .collect();

        let kg = words
            .iter()
            .find_map(|word| weight_kg(word))
            .ok_or("I need a weight, e.g. `quote 2.5kg to eu`.")?;
        let zone = words
            .iter()
            .find_map(|word| RATES.iter().find(|(zone, ..)| zone == word).map(|r| r.0))
            .ok_or_else(|| {
                let zones: Vec<&str> = RATES.iter().map(|(zone, ..)| *zone).collect();
                format!("I need a zone — one of {}.", zones.join(", "))
            })?;

        if kg <= 0.0 {
            return Err(format!("{kg}kg is not a shipment I can quote."));
        }
        if kg > MAX_KG {
            return Err(format!(
                "{kg}kg is over the {MAX_KG}kg parcel limit — that ships as freight, \
                 which is quoted by hand."
            ));
        }
        Ok(Shipment { kg, zone })
    }

    /// What it costs, to the cent.
    fn quote(&self) -> f64 {
        let (_, handling, per_kg) = RATES
            .iter()
            .find(|(zone, ..)| *zone == self.zone)
            .expect("zone came from RATES");
        ((handling + per_kg * self.kg) * 100.0).round() / 100.0
    }
}

/// The handler: one skill, computed rather than generated.
#[derive(Clone)]
struct ShippingQuoteHandler;

#[async_trait]
impl AsyncMessageHandler for ShippingQuoteHandler {
    async fn process_message(
        &self,
        task_id: &str,
        message: &Message,
        _ctx: &RequestContext,
    ) -> Result<Task, A2AError> {
        let text = message
            .parts
            .iter()
            .find_map(|part| part.get_text())
            .unwrap_or_default();

        // A request this agent cannot price ends `input-required`, not
        // `completed` with an apology: the caller — a person or an orchestrating
        // model — can then answer the question instead of relaying a guess.
        let (state, reply) = match Shipment::parse(text) {
            Ok(shipment) => (
                TaskState::Completed,
                format!(
                    "{:.1}kg to {}: {:.2} EUR",
                    shipment.kg,
                    shipment.zone,
                    shipment.quote()
                ),
            ),
            Err(why) => (TaskState::InputRequired, why),
        };

        let response = Message::builder()
            .role(Role::Agent)
            .parts(vec![Part::text(reply)])
            .message_id(Uuid::new_v4().to_string())
            .context_id(message.context_id.clone())
            .build();

        Ok(Task::builder()
            .id(task_id.to_string())
            .context_id(message.context_id.clone())
            .status(TaskStatus::new(state, Some(response.clone())))
            .history(vec![message.clone(), response])
            .build())
    }
}

#[tokio::main]
async fn main() -> Result<(), BuildError> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let config = env::var(CONFIG_ENV).unwrap_or_else(|_| LOCAL_CONFIG.to_string());
    println!("shipping-quote agent, config: {config}");

    // The config still decides everything it decided before — name, card,
    // skills, port, storage. This binary only supplies the handler.
    AgentBuilder::from_file(&config)?
        .with_handler(ShippingQuoteHandler)
        .build_with_auto_storage()
        .await?
        .run()
        .await
        .map_err(|e| BuildError::RuntimeError(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quote_is_handling_plus_rate_times_weight() {
        let shipment = Shipment::parse("quote 2.5kg to eu").expect("parses");
        assert_eq!(shipment.zone, "eu");
        // 9.00 + 2.40 * 2.5
        assert_eq!(shipment.quote(), 15.00);
    }

    #[test]
    fn word_order_and_noise_do_not_matter() {
        for text in [
            "how much to send 2.5 kg to the eu?",
            "EU, 2.5kg please",
            "quote: zone eu weight 2.5",
        ] {
            let shipment = Shipment::parse(text).unwrap_or_else(|e| panic!("{text:?}: {e}"));
            assert_eq!((shipment.zone, shipment.kg), ("eu", 2.5));
        }
    }

    /// Grams are converted, not rejected — half the ways to say "under a kilo"
    /// use them, and a quote that ignored the unit would be off by 1000×.
    #[test]
    fn grams_are_a_weight_too() {
        let shipment = Shipment::parse("quote 800g domestic").expect("parses");
        assert_eq!(shipment.kg, 0.8);
        // 4.50 + 1.20 * 0.8
        assert_eq!(shipment.quote(), 5.46);
    }

    /// What the agent will not answer, and why each is a question rather than a
    /// guess.
    #[test]
    fn unpriceable_requests_say_what_is_missing() {
        for (text, expected) in [
            ("ship this to eu", "weight"),
            ("2.5kg to mars", "zone"),
            ("40kg to eu", "freight"),
        ] {
            let why = Shipment::parse(text).err().unwrap_or_else(|| {
                panic!("{text:?} should not be priceable");
            });
            assert!(why.contains(expected), "{text:?} said: {why}");
        }
    }
}
