use crate::domain::messages::{ChatGPTMessage, ChatGPTRequest, ChatGPTResponse};
use crate::domain::models::SeriesDetails;
use crate::domain::traits::JsonFetcher;
use anyhow::anyhow;
use std::sync::Arc;

pub type ChatFetcher = Arc<dyn for<'a> JsonFetcher<'a, ChatGPTResponse, ChatGPTRequest>>;

pub struct ChatGPT {
    client: ChatFetcher,
}

const CHAT_MODEL: &str = "gpt-4";

const CHAT_URL: &str = "https://api.openai.com/v1/chat/completions";

const PARSE_SYSTEM_MSG: &str = "Parse Series Title, Season, Episode and
Episode Title of TV series from a file name, giving the response as machine
readable JSON list.";

const PARSE_SAMPLE_REQUEST: &str = "Line Of Duty/Line Of Duty S02E02.mp4
Only Fools and Horses/Specials/S00E03 - Diamonds Are for Heather.mkv
Your a boat john [jLKJOL8&*UYG].webm";

const PARSE_SAMPLE_RESPONSE: &str = "[
    {
        \"seriesTitle\": \"Line Of Duty\",
        \"season\": \"2\",
        \"episode\": \"2\",
        \"episodeTitle\": \"\"
    },
    {
        \"seriesTitle\": \"Only Fools and Horses\",
        \"season\": \"Specials\",
        \"episode\": \"3\",
        \"episodeTitle\": \"Diamonds Are Heather\"
    },
    {
        \"seriesTitle\": \"You are a boat John\",
        \"season\": \"\",
        \"episode\": \"\",
        \"episodeTitle\": \"\"
    }
]";

impl ChatGPT {
    pub fn new(client: ChatFetcher) -> Self {
        Self { client }
    }

    pub async fn describe_video(&self, video: &SeriesDetails) -> anyhow::Result<String> {
        let message = format!(
            "Create an imaginary review for a film with the following title: {}",
            video.full_title()
        );

        let messages = vec![ChatGPTMessage::user(&message)];

        let request = ChatGPTRequest {
            model: CHAT_MODEL.to_string(),
            messages: messages,
            ..Default::default()
        };

        let response = self.client.fetch(CHAT_URL, &request).await?;

        Ok(response.get_all_content())
    }

    pub async fn parse_filename(&self, video: &str) -> anyhow::Result<SeriesDetails> {
        let messages = vec![
            ChatGPTMessage::system(PARSE_SYSTEM_MSG),
            ChatGPTMessage::user(PARSE_SAMPLE_REQUEST),
            ChatGPTMessage::assistant(PARSE_SAMPLE_RESPONSE),
            ChatGPTMessage::user(video),
        ];

        let request = ChatGPTRequest {
            model: CHAT_MODEL.to_string(),
            messages: messages,
            max_tokens: Some(1024),
            ..Default::default()
        };

        let response = self.client.fetch(CHAT_URL, &request).await?;

        let content = strip_file_paths(&response.get_all_content());

        match serde_json::from_str::<Vec<SeriesDetails>>(&content) {
            Ok(result_list) => {
                if result_list.is_empty() {
                    Err(anyhow!("could not parse video name {}", video))
                } else {
                    Ok(result_list.get(0).unwrap().to_owned())
                }
            }
            Err(e) => Err(e.into()),
        }
    }
}

fn strip_file_paths(response: &str) -> String {
    let lines: Vec<&str> = response.trim().split('\n').collect();
    let mut new_lines: Vec<String> = Vec::new();

    for line in lines {
        let line = line.trim();
        if !line.is_empty()
            && (line.chars().nth(0) == Some('{')
                || line.chars().nth(0) == Some('}')
                || line.chars().nth(0) == Some('"'))
        {
            let mut line = line.to_string();
            if line == "}" {
                line = "},".to_string();
            }

            new_lines.push(line);
        }
    }

    if !new_lines.is_empty() {
        let last_index = new_lines.len() - 1;
        new_lines[last_index] = "}".to_string();
    }

    let new_lines: String = new_lines.join("\n");

    format!("[\n{}\n]", new_lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptors::HTTPClient;
    use anyhow::Result;

    #[test]
    fn test_empty_input() {
        let input = "";
        let expected = "[\n\n]";
        assert_eq!(strip_file_paths(input), expected);
    }

    #[test]
    fn test_single_valid_line() {
        let input = r#"
            {
                "name": "file.txt",
                "path": "/home/user/file.txt"
            }
        "#;
        let expected = "[\n{\n\"name\": \"file.txt\",\n\"path\": \"/home/user/file.txt\"\n}\n]";
        assert_eq!(strip_file_paths(input), expected);
    }

    #[test]
    fn test_multiple_valid_lines() {
        let input = r#"
            {
                "name": "file1.txt",
                "path": "/home/user/file1.txt"
            }
            {
                "name": "file2.txt",
                "path": "/home/user/file2.txt"
            }
        "#;
        let expected = "[\n{\n\"name\": \"file1.txt\",\n\"path\": \"/home/user/file1.txt\"\n},\n{\n\"name\": \"file2.txt\",\n\"path\": \"/home/user/file2.txt\"\n}\n]";
        assert_eq!(strip_file_paths(input), expected);
    }

    #[test]
    fn test_invalid_lines_ignored() {
        let input = r#"
            some invalid text
            {
                "name": "file.txt",
                "path": "/home/user/file.txt"
            }
            more invalid text
        "#;
        let expected = "[\n{\n\"name\": \"file.txt\",\n\"path\": \"/home/user/file.txt\"\n}\n]";
        assert_eq!(strip_file_paths(input), expected);
    }

    #[tokio::test]
    #[ignore]
    async fn test_chatgpt_parse_filename() -> Result<()> {
        let client: ChatFetcher = Arc::new(HTTPClient::new());

        let chatgpt = ChatGPT { client };

        let series = chatgpt
            .parse_filename("The Sweeney/Season 4/The Sweeney 4-08 The Bigger They Are.mkv")
            .await?;

        assert_eq!(series.series_title, "The Sweeney");
        assert_eq!(series.season, "4");
        assert_eq!(series.episode, "8");
        assert_eq!(series.episode_title, "The Bigger They Are");

        let series = chatgpt.parse_filename("Not much info.mkv").await?;

        assert_eq!(series.series_title, "Not much info");
        assert_eq!(series.season, "");
        assert_eq!(series.episode, "");
        assert_eq!(series.episode_title, "");

        Ok(())
    }

    #[tokio::test]
    #[ignore]
    async fn test_chatgpt_describe() -> Result<()> {
        let client: ChatFetcher = Arc::new(HTTPClient::new());

        let chatgpt = ChatGPT::new(client);

        let series = SeriesDetails {
            series_title: "The Sweeney".to_string(),
            season: "4".to_string(),
            episode: "8".to_string(),
            episode_title: "The Bigger They Are".to_string(),
        };

        let description = chatgpt.describe_video(&series).await?;

        assert!(!description.is_empty());

        Ok(())
    }
}
