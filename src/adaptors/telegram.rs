use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::Serialize;

use crate::domain::traits::InstantMessegeService;

pub struct TelegramBot {
    channel_id: String,
    bot_token: String,
    client: Client,
}

const TELEGRAM_API: &str = "https://api.telegram.org/bot";
const SEND_ENDPOINT: &str = "/sendMessage";


#[derive(Serialize)]
struct TelegramMessage<'a> {
    chat_id: &'a str,
    text: &'a str,
    parse_mode: &'a str,
}

impl TelegramBot {
    pub fn new(channel_id: &str, bot_token: &str) -> Self {
        Self {
            channel_id: channel_id.to_string(),
            bot_token: bot_token.to_string(),
            client: Client::new(),
        }
    }

    async fn send(&self, message: &str, mode: &str) -> Result<()> {
        let api_url = format!("{}{}{}", TELEGRAM_API, self.bot_token, SEND_ENDPOINT);

        // Create the message payload
        let payload = TelegramMessage {
            chat_id: &self.channel_id,
            text: message,
            parse_mode: mode,
        };

        // Send POST request to the Telegram API
        let response = self.client.post(&api_url)
            .json(&payload)
            .send()
            .await?;

        // Check response status
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await?;
            return Err(anyhow!("Unexpected status from Telegram API: {}, body: {}", status, body));
        }

        Ok(())
    }
}

#[async_trait]
impl InstantMessegeService for TelegramBot {
    async fn send_error(&self, video: String, error: &str) -> Result<()> {
        let message = format!("Could not share {}, the error was {}", video, error);
        self.send(&message, "HTML").await
    }

    async fn send_link(&self, title: &str, link: &str) -> Result<()> {
        let message = format!("<a href=\"{}\">{}</a>", link, title);
        self.send(&message, "HTML").await
    }
}
