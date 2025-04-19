use serde::{Deserialize, Serialize};
use std::collections::HashMap;


#[serde_with::skip_serializing_none]
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatGPTRequest {
    pub model: String,
    pub messages: Vec<ChatGPTMessage>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub n: Option<i32>,
    pub stream: Option<bool>,
    pub stop: Option<Vec<String>>,
    pub max_tokens: Option<i32>,
    pub presence_penalty: Option<f64>,
    pub frequency_penalty: Option<f64>,
    pub logit_bias: Option<HashMap<String, f64>>,
    pub user: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatGPTResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub usage: ChatGPTUsage,
    pub choices: Vec<ChatGPTChoice>,
}

impl<'a> ChatGPTResponse {
    pub fn get_all_content(&self) -> String {
        self.choices
            .iter()
            .map(|c| c.message.content.to_string())
            .collect::<Vec<String>>()
            .join(" ")
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatGPTUsage {
    #[serde(rename = "prompt_tokens")]
    pub prompt_tokens: i64,
    #[serde(rename = "completion_tokens")]
    pub completion_tokens: i64,
    #[serde(rename = "total_tokens")]
    pub total_tokens: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatGPTChoice {
    pub message: ChatGPTMessage,
    #[serde(rename = "finish_reason")]
    pub finish_reason: String,
    pub index: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatGPTMessage {
    pub role: String,
    pub content: String,
}

impl ChatGPTMessage {
    pub fn system(message: &str) -> Self {
        Self {
            role: "system".to_string(),
            content: message.to_string(),
        }
    }

    pub fn user(message: &str) -> Self {
        Self {
            role: "user".to_string(),
            content: message.to_string(),
        }
    }

    pub fn assistant(message: &str) -> Self {
        Self {
            role: "assistant".to_string(),
            content: message.to_string(),
        }
    }
}
