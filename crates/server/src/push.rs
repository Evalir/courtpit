//! Push notifications behind a trait: [`LogPusher`] for dev/test, [`ExpoPusher`] for
//! production (Expo's push service relays to APNs and FCM; spec §14).

use std::{pin::Pin, sync::Mutex};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Boxed future returned by [`Pusher::send`] (keeps the trait object-safe).
pub type PushFuture<'a> = Pin<Box<dyn Future<Output = anyhow::Result<Vec<Delivery>>> + Send + 'a>>;

/// One notification for one device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PushMessage {
    /// The device's Expo push token.
    pub to: String,
    /// Bold first line.
    pub title: String,
    /// The text under it.
    pub body: String,
    /// Delivered to the app with the notification: `{ "url": "/matches/<id>" }`.
    pub data: Value,
}

/// What became of one message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// Accepted for delivery.
    Sent,
    /// The token no longer reaches an installed app; forget it.
    Unregistered,
    /// Not accepted for another reason (logged); the token stays.
    Failed,
}

/// Sends push notifications.
pub trait Pusher: Send + Sync + std::fmt::Debug {
    /// Sends `messages`; the result has one [`Delivery`] per message, in order.
    fn send<'a>(&'a self, messages: &'a [PushMessage]) -> PushFuture<'a>;
}

/// Logs messages and keeps them in memory (tests read [`LogPusher::sent`]). Tokens containing
/// `unregistered` are reported as such, so tests can exercise pruning.
#[derive(Debug, Default)]
pub struct LogPusher {
    outbox: Mutex<Vec<PushMessage>>,
}

impl LogPusher {
    /// All messages sent so far.
    pub fn sent(&self) -> Vec<PushMessage> {
        self.outbox
            .lock()
            .map(|outbox| outbox.clone())
            .unwrap_or_default()
    }
}

impl Pusher for LogPusher {
    fn send<'a>(&'a self, messages: &'a [PushMessage]) -> PushFuture<'a> {
        Box::pin(async move {
            let mut deliveries = Vec::with_capacity(messages.len());
            for message in messages {
                tracing::info!(to = %message.to, title = %message.title, body = %message.body, "push (log pusher)");
                if let Ok(mut outbox) = self.outbox.lock() {
                    outbox.push(message.clone());
                }
                deliveries.push(if message.to.contains("unregistered") {
                    Delivery::Unregistered
                } else {
                    Delivery::Sent
                });
            }
            Ok(deliveries)
        })
    }
}

/// Sends through Expo's push API (`POST https://exp.host/--/api/v2/push/send`, up to 100
/// messages a request). An access token is optional unless the Expo project requires one.
#[derive(Debug, Clone)]
pub struct ExpoPusher {
    client: reqwest::Client,
    access_token: Option<String>,
    endpoint: String,
}

impl ExpoPusher {
    /// Creates a pusher, authenticating with `access_token` when given.
    pub fn new(access_token: Option<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            access_token,
            endpoint: "https://exp.host/--/api/v2/push/send".to_owned(),
        }
    }

    /// Overrides the API endpoint (tests point it at a local server).
    #[must_use]
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    async fn send_chunk(&self, chunk: &[PushMessage]) -> anyhow::Result<Vec<Delivery>> {
        let mut request = self.client.post(&self.endpoint).json(chunk);
        if let Some(token) = &self.access_token {
            request = request.bearer_auth(token);
        }
        let res = request.send().await.context("calling Expo push")?;
        let status = res.status();
        if !status.is_success() {
            let body = res.text().await.unwrap_or_default();
            anyhow::bail!("Expo push returned {status}: {body}");
        }
        let tickets: Tickets = res.json().await.context("reading Expo push tickets")?;
        anyhow::ensure!(
            tickets.data.len() == chunk.len(),
            "Expo push returned {} tickets for {} messages",
            tickets.data.len(),
            chunk.len()
        );
        Ok(tickets.data.iter().map(Ticket::delivery).collect())
    }
}

#[derive(Deserialize)]
struct Tickets {
    data: Vec<Ticket>,
}

#[derive(Deserialize)]
struct Ticket {
    status: String,
    message: Option<String>,
    details: Option<TicketDetails>,
}

#[derive(Deserialize)]
struct TicketDetails {
    error: Option<String>,
}

impl Ticket {
    fn delivery(&self) -> Delivery {
        if self.status == "ok" {
            return Delivery::Sent;
        }
        let error = self
            .details
            .as_ref()
            .and_then(|details| details.error.as_deref());
        if error == Some("DeviceNotRegistered") {
            return Delivery::Unregistered;
        }
        tracing::warn!(?error, message = ?self.message, "push not accepted");
        Delivery::Failed
    }
}

/// Expo's limit of messages per request.
const CHUNK: usize = 100;

impl Pusher for ExpoPusher {
    fn send<'a>(&'a self, messages: &'a [PushMessage]) -> PushFuture<'a> {
        Box::pin(async move {
            let mut deliveries = Vec::with_capacity(messages.len());
            for chunk in messages.chunks(CHUNK) {
                deliveries.extend(self.send_chunk(chunk).await?);
            }
            Ok(deliveries)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{Json, Router, extract::State, http::HeaderMap, routing::post};
    use serde_json::json;

    use super::*;

    fn message(to: &str) -> PushMessage {
        PushMessage {
            to: to.to_owned(),
            title: "Score to confirm".to_owned(),
            body: "Ana reported 6–4 6–3".to_owned(),
            data: json!({ "url": "/matches/m1" }),
        }
    }

    type Seen = Arc<Mutex<Vec<(Option<String>, Value)>>>;

    async fn fake_expo() -> (String, Seen) {
        let seen: Seen = Arc::default();
        let app = Router::new()
            .route(
                "/push",
                post(
                    async move |State(seen): State<Seen>,
                                headers: HeaderMap,
                                Json(body): Json<Value>| {
                        let auth = headers
                            .get("authorization")
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_owned);
                        let tickets: Vec<Value> = body
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|message| {
                                if message["to"].as_str().unwrap().contains("gone") {
                                    json!({ "status": "error", "message": "not registered",
                                            "details": { "error": "DeviceNotRegistered" } })
                                } else {
                                    json!({ "status": "ok", "id": "ticket" })
                                }
                            })
                            .collect();
                        seen.lock().unwrap().push((auth, body));
                        Json(json!({ "data": tickets }))
                    },
                ),
            )
            .with_state(seen.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap()
        }));
        (format!("http://{addr}/push"), seen)
    }

    #[tokio::test]
    async fn expo_sends_messages_and_reads_tickets() {
        let (url, seen) = fake_expo().await;
        let pusher = ExpoPusher::new(Some("secret".to_owned())).with_endpoint(url);
        let deliveries = pusher
            .send(&[
                message("ExponentPushToken[a]"),
                message("ExponentPushToken[gone]"),
            ])
            .await
            .unwrap();
        assert_eq!(deliveries, [Delivery::Sent, Delivery::Unregistered]);
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0].0.as_deref(), Some("Bearer secret"));
        assert_eq!(seen[0].1[0]["data"]["url"], "/matches/m1");
        assert_eq!(seen[0].1[0]["title"], "Score to confirm");
    }

    #[tokio::test]
    async fn expo_sends_in_chunks_of_a_hundred() {
        let (url, seen) = fake_expo().await;
        let pusher = ExpoPusher::new(None).with_endpoint(url);
        let messages: Vec<PushMessage> = std::iter::repeat_with(|| message("ExponentPushToken[a]"))
            .take(150)
            .collect();
        let deliveries = pusher.send(&messages).await.unwrap();
        assert_eq!(deliveries.len(), 150);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(seen[0].0.is_none());
        assert_eq!(seen[1].1.as_array().map(Vec::len), Some(50));
    }

    #[tokio::test]
    async fn the_log_pusher_records_and_flags_unregistered_tokens() {
        let pusher = LogPusher::default();
        let deliveries = pusher
            .send(&[message("a"), message("unregistered-b")])
            .await
            .unwrap();
        assert_eq!(deliveries, [Delivery::Sent, Delivery::Unregistered]);
        assert_eq!(pusher.sent().len(), 2);
    }
}
