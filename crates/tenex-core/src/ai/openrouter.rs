use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const OPENROUTER_API_BASE: &str = "https://openrouter.ai/api/v1";

/// Represents a model from OpenRouter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub context_length: Option<u32>,
    pub pricing: Option<Pricing>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pricing {
    pub prompt: Option<String>,
    pub completion: Option<String>,
}

/// Response from OpenRouter models API
#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<Model>,
}

/// OpenRouter API client
pub struct OpenRouterClient {
    api_key: String,
    client: reqwest::Client,
}

impl OpenRouterClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
        }
    }

    /// Fetch all available models from OpenRouter
    pub async fn get_models(&self) -> Result<Vec<Model>> {
        let url = format!("{}/models", OPENROUTER_API_BASE);

        let response = self.client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .context("Failed to send request to OpenRouter API")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("OpenRouter API error ({}): {}", status, error_text);
        }

        let models_response: ModelsResponse = response
            .json()
            .await
            .context("Failed to parse OpenRouter models response")?;

        Ok(models_response.data)
    }

    /// Convert text to audio-friendly format using the specified model
    pub async fn massage_text_for_audio(
        &self,
        text: &str,
        conversation_title: Option<&str>,
        model: &str,
        custom_prompt: &str,
    ) -> Result<String> {
        let url = format!("{}/chat/completions", OPENROUTER_API_BASE);

        let context = if let Some(title) = conversation_title {
            format!("Conversation: {}\n\n", title)
        } else {
            String::new()
        };

        let user_message = format!("{}{}", context, text);

        let body = serde_json::json!({
            "model": model,
            "messages": [
                {
                    "role": "system",
                    "content": custom_prompt
                },
                {
                    "role": "user",
                    "content": user_message
                }
            ],
        });

        let response = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to send chat completion request")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("OpenRouter chat completion error ({}): {}", status, error_text);
        }

        let response_json: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse OpenRouter chat response")?;

        let massaged_text = response_json["choices"][0]["message"]["content"]
            .as_str()
            .context("Failed to extract message content from response")?
            .to_string();

        Ok(massaged_text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires actual API key
    async fn test_get_models() {
        let api_key = std::env::var("OPENROUTER_API_KEY").expect("OPENROUTER_API_KEY not set");
        let client = OpenRouterClient::new(api_key);

        let models = client.get_models().await.unwrap();
        assert!(!models.is_empty());
    }

    #[tokio::test]
    #[ignore] // Requires actual API key
    async fn test_massage_text() {
        let api_key = std::env::var("OPENROUTER_API_KEY").expect("OPENROUTER_API_KEY not set");
        let client = OpenRouterClient::new(api_key);

        let text = "```rust\nfn main() {\n    println!(\"Hello\");\n}\n```\nCheck out this code!";
        let prompt = "Remove code blocks and make this audio-friendly.";

        let result = client
            .massage_text_for_audio(text, Some("Test Conversation"), "openai/gpt-3.5-turbo", prompt)
            .await
            .unwrap();

        assert!(!result.is_empty());
        assert!(!result.contains("```")); // Code blocks should be removed
    }
}
