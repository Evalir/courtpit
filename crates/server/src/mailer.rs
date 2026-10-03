//! Outbound email behind a trait: [`LogMailer`] for dev/test, [`ResendMailer`] for production.

use std::{pin::Pin, sync::Mutex};

use anyhow::Context;
use serde::Serialize;

/// Boxed future returned by [`Mailer::send`] (keeps the trait object-safe).
pub type MailFuture<'a> = Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>>;

/// A plain-text email.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Email {
    /// Recipient address.
    pub to: String,
    /// Subject line.
    pub subject: String,
    /// Plain-text body.
    pub text: String,
}

/// Sends email.
pub trait Mailer: Send + Sync + std::fmt::Debug {
    /// Sends one message.
    fn send<'a>(&'a self, email: &'a Email) -> MailFuture<'a>;
}

/// Logs messages and keeps them in memory (tests read codes from [`LogMailer::sent`]).
#[derive(Debug, Default)]
pub struct LogMailer {
    outbox: Mutex<Vec<Email>>,
}

impl LogMailer {
    /// All messages sent so far.
    pub fn sent(&self) -> Vec<Email> {
        self.outbox
            .lock()
            .map(|outbox| outbox.clone())
            .unwrap_or_default()
    }

    /// The most recent message to `to` (case-insensitive).
    pub fn last_to(&self, to: &str) -> Option<Email> {
        self.sent()
            .into_iter()
            .rev()
            .find(|email| email.to.eq_ignore_ascii_case(to))
    }
}

impl Mailer for LogMailer {
    fn send<'a>(&'a self, email: &'a Email) -> MailFuture<'a> {
        Box::pin(async move {
            tracing::info!(to = %email.to, subject = %email.subject, body = %email.text, "email (log mailer)");
            if let Ok(mut outbox) = self.outbox.lock() {
                outbox.push(email.clone());
            }
            Ok(())
        })
    }
}

/// Sends through the Resend HTTP API (`POST https://api.resend.com/emails`). No SDK needed.
#[derive(Debug, Clone)]
pub struct ResendMailer {
    client: reqwest::Client,
    api_key: String,
    from: String,
    endpoint: String,
}

impl ResendMailer {
    /// Creates a mailer sending as `from` (e.g. `Courtpit <no-reply@courtpit.app>`).
    pub fn new(api_key: String, from: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            from,
            endpoint: "https://api.resend.com/emails".to_owned(),
        }
    }

    /// Overrides the API endpoint (tests point it at a local server).
    #[must_use]
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }
}

#[derive(Serialize)]
struct ResendBody<'a> {
    from: &'a str,
    to: [&'a str; 1],
    subject: &'a str,
    text: &'a str,
}

impl Mailer for ResendMailer {
    fn send<'a>(&'a self, email: &'a Email) -> MailFuture<'a> {
        Box::pin(async move {
            let res = self
                .client
                .post(&self.endpoint)
                .bearer_auth(&self.api_key)
                .json(&ResendBody {
                    from: &self.from,
                    to: [&email.to],
                    subject: &email.subject,
                    text: &email.text,
                })
                .send()
                .await
                .context("calling Resend")?;
            let status = res.status();
            if !status.is_success() {
                let body = res.text().await.unwrap_or_default();
                anyhow::bail!("Resend returned {status}: {body}");
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{Json, Router, extract::State, http::HeaderMap, routing::post};
    use serde_json::Value;

    use super::*;

    fn email() -> Email {
        Email {
            to: "Ana@Example.test".into(),
            subject: "Your code".into(),
            text: "123456".into(),
        }
    }

    #[tokio::test]
    async fn log_mailer_keeps_an_outbox() {
        let mailer = LogMailer::default();
        mailer.send(&email()).await.unwrap();
        assert_eq!(mailer.sent().len(), 1);
        assert_eq!(mailer.last_to("ana@example.test").unwrap().text, "123456");
        assert!(mailer.last_to("bob@example.test").is_none());
    }

    type Seen = Arc<Mutex<Vec<(Option<String>, Value)>>>;

    async fn fake_resend(status: u16) -> (String, Seen) {
        let seen: Seen = Arc::default();
        let app = Router::new()
            .route(
                "/emails",
                post(
                    async move |State(seen): State<Seen>,
                                headers: HeaderMap,
                                Json(body): Json<Value>| {
                        let auth = headers
                            .get("authorization")
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_owned);
                        seen.lock().unwrap().push((auth, body));
                        axum::http::StatusCode::from_u16(status).unwrap()
                    },
                ),
            )
            .with_state(seen.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap()
        }));
        (format!("http://{addr}/emails"), seen)
    }

    #[tokio::test]
    async fn resend_mailer_posts_the_message() {
        let (endpoint, seen) = fake_resend(200).await;
        let mailer = ResendMailer::new("re_key".into(), "Courtpit <no-reply@x.test>".into())
            .with_endpoint(endpoint);
        mailer.send(&email()).await.unwrap();
        let seen = seen.lock().unwrap();
        let (auth, body) = &seen[0];
        assert_eq!(auth.as_deref(), Some("Bearer re_key"));
        assert_eq!(body["to"], serde_json::json!(["Ana@Example.test"]));
        assert_eq!(body["from"], "Courtpit <no-reply@x.test>");
        assert_eq!(body["text"], "123456");
    }

    #[tokio::test]
    async fn resend_errors_surface() {
        let (endpoint, _) = fake_resend(422).await;
        let mailer = ResendMailer::new("k".into(), "f@x.test".into()).with_endpoint(endpoint);
        let err = mailer.send(&email()).await.unwrap_err();
        assert!(err.to_string().contains("422"), "{err}");
    }
}
