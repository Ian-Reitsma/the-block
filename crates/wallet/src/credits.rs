use reqwest::blocking::Client;
use serde::Serialize;

/// Notifies registered endpoints when credit events occur.
#[derive(Default)]
pub struct CreditNotifier {
    endpoints: Vec<String>,
    client: Client,
}

impl CreditNotifier {
    /// Register a webhook or push endpoint that will receive credit events.
    pub fn register_webhook(&mut self, url: impl Into<String>) {
        self.endpoints.push(url.into());
    }

    /// Trigger a notification for a balance change.
    pub fn notify_balance_change(
        &self,
        provider: &str,
        balance: u64,
    ) -> Result<(), reqwest::Error> {
        #[derive(Serialize)]
        struct Payload<'a> { provider: &'a str, balance: u64 }
        let payload = Payload { provider, balance };
        for ep in &self.endpoints {
            let _ = self.client.post(ep).json(&payload).send()?;
        }
        Ok(())
    }

    /// Trigger a notification indicating the client hit a rate limit.
    pub fn notify_rate_limit(&self, provider: &str) -> Result<(), reqwest::Error> {
        #[derive(Serialize)]
        struct Payload<'a> { provider: &'a str, event: &'static str }
        let payload = Payload { provider, event: "rate_limit" };
        for ep in &self.endpoints {
            let _ = self.client.post(ep).json(&payload).send()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::{Method::POST, MockServer};

    #[test]
    fn sends_notifications() {
        let server = MockServer::start();
        let balance = server.mock(|when, then| {
            when.method(POST).path("/bal");
            then.status(200);
        });
        let rate = server.mock(|when, then| {
            when.method(POST).path("/rate");
            then.status(200);
        });
        let mut notif = CreditNotifier::default();
        notif.register_webhook(server.url("/bal"));
        notif.register_webhook(server.url("/rate"));
        notif.notify_balance_change("p", 10).unwrap();
        notif.notify_rate_limit("p").unwrap();
        assert_eq!(balance.hits(), 2);
        assert_eq!(rate.hits(), 2);
    }
}

