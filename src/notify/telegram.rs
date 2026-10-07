//! Telegram (Bot API `sendMessage`), in plain text: no parse mode, so the
//! text reaches the reader as written and nothing needs escaping. The limit
//! is 4096 UTF-16 code units.

use serde_json::{Value, json};

use super::Notifier;
use crate::http::{HttpRequest, Transport};

pub const MAX_LEN: usize = 4096;
const API: &str = "https://api.telegram.org";

pub struct Telegram {
    token: String,
    chat: String,
    transport: Box<dyn Transport>,
    api: String,
}

impl Telegram {
    pub fn new(token: &str, chat: &str, transport: Box<dyn Transport>) -> Telegram {
        Telegram::with_api(token, chat, transport, API)
    }

    pub fn with_api(token: &str, chat: &str, transport: Box<dyn Transport>, api: &str) -> Telegram {
        Telegram {
            token: token.to_string(),
            chat: chat.to_string(),
            transport,
            api: api.trim_end_matches('/').to_string(),
        }
    }
}

impl Notifier for Telegram {
    fn name(&self) -> String {
        "telegram".into()
    }

    fn max_len(&self) -> usize {
        MAX_LEN
    }

    fn measure(&self, text: &str) -> usize {
        text.encode_utf16().count()
    }

    fn send(&self, text: &str) -> Result<(), String> {
        if text.trim().is_empty() {
            return Err("refusing to send an empty message".into());
        }
        let body = json!({"chat_id": self.chat, "text": text, "disable_web_page_preview": true});
        let response = self
            .transport
            .send(HttpRequest {
                method: "POST",
                url: format!("{}/bot{}/sendMessage", self.api, self.token),
                headers: vec![("Content-Type".into(), "application/json".into())],
                body: Some(body.to_string().into_bytes()),
            })
            // The URL carries the token: never repeat the transport's message.
            .map_err(|_| "Telegram could not be reached".to_string())?;
        let v: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
        if response.status == 200 && v["ok"].as_bool() == Some(true) {
            Ok(())
        } else {
            Err(format!(
                "Telegram refused the message (HTTP {}: {})",
                response.status,
                v["description"].as_str().unwrap_or("no description")
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::ScriptedTransport;
    use std::sync::Arc;

    struct Shared(Arc<ScriptedTransport>);
    impl Transport for Shared {
        fn send(&self, r: HttpRequest) -> Result<crate::http::HttpResponse, String> {
            self.0.send(r)
        }
    }

    fn telegram() -> (Telegram, Arc<ScriptedTransport>) {
        let t = Arc::new(ScriptedTransport::new());
        (
            Telegram::with_api(
                "test-token",
                "chat-ref",
                Box::new(Shared(t.clone())),
                "https://tg.test",
            ),
            t,
        )
    }

    #[test]
    fn a_message_is_sent_as_plain_text() {
        let (tg, t) = telegram();
        t.on(
            "POST",
            "https://tg.test/bottest-token/sendMessage",
            200,
            r#"{"ok":true}"#,
        );
        tg.send("acme 1.2.3 is published. *not bold* _nor italic_")
            .unwrap();
        let body: Value = serde_json::from_str(&t.bodies()[0]).unwrap();
        assert_eq!(
            body["text"],
            "acme 1.2.3 is published. *not bold* _nor italic_"
        );
        assert!(
            body.get("parse_mode").is_none(),
            "plain text: nothing to escape"
        );
        assert_eq!(body["chat_id"], "chat-ref");
    }

    #[test]
    fn a_refused_send_is_reported_not_claimed() {
        let (tg, t) = telegram();
        t.on(
            "POST",
            "https://tg.test/bottest-token/sendMessage",
            400,
            r#"{"ok":false,"description":"Bad Request: chat not found"}"#,
        );
        let err = tg.send("x").unwrap_err();
        assert!(err.contains("chat not found"));
        assert!(
            !err.contains("test-token"),
            "the token never appears in an error"
        );
    }

    #[test]
    fn an_empty_message_is_refused() {
        let (tg, _) = telegram();
        assert!(tg.send("  ").is_err());
    }

    #[test]
    fn length_is_measured_in_utf16_units() {
        let (tg, _) = telegram();
        assert_eq!(tg.measure("a😀"), 3);
    }
}
