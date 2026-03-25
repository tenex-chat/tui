use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct TelegramClient {
    http: reqwest::Client,
    bot_token: String,
}

impl TelegramClient {
    pub fn new(bot_token: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            bot_token,
        }
    }

    pub async fn get_me(&self) -> Result<TelegramUser> {
        self.get("getMe").await
    }

    pub async fn set_webhook(&self, url: &str, secret_token: &str) -> Result<bool> {
        #[derive(Serialize)]
        struct Payload<'a> {
            url: &'a str,
            secret_token: &'a str,
            allowed_updates: [&'a str; 3],
        }

        self.post(
            "setWebhook",
            &Payload {
                url,
                secret_token,
                allowed_updates: ["message", "my_chat_member", "callback_query"],
            },
        )
        .await
    }

    pub async fn send_message(&self, request: SendMessageRequest) -> Result<TelegramMessage> {
        self.post("sendMessage", &request).await
    }

    pub async fn edit_message_text(
        &self,
        request: EditMessageTextRequest,
    ) -> Result<TelegramMessage> {
        self.post("editMessageText", &request).await
    }

    pub async fn answer_callback_query(&self, request: AnswerCallbackQueryRequest) -> Result<bool> {
        self.post("answerCallbackQuery", &request).await
    }

    async fn get<T>(&self, method: &str) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let response = self
            .http
            .get(self.api_url(method))
            .send()
            .await
            .with_context(|| format!("Telegram request failed for {method}"))?;
        parse_response(method, response).await
    }

    async fn post<TReq, TResp>(&self, method: &str, body: &TReq) -> Result<TResp>
    where
        TReq: Serialize + ?Sized,
        TResp: for<'de> Deserialize<'de>,
    {
        let response = self
            .http
            .post(self.api_url(method))
            .json(body)
            .send()
            .await
            .with_context(|| format!("Telegram request failed for {method}"))?;
        parse_response(method, response).await
    }

    fn api_url(&self, method: &str) -> String {
        format!("https://api.telegram.org/bot{}/{}", self.bot_token, method)
    }
}

async fn parse_response<T>(method: &str, response: reqwest::Response) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    parse_response_body(method, status.as_u16(), status.is_success(), &body)
}

fn parse_response_body<T>(method: &str, status_code: u16, is_success: bool, body: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let decoded: TelegramApiResponse<T> = serde_json::from_str(&body)
        .with_context(|| format!("Failed to parse Telegram response for {method}: {body}"))?;

    if !is_success || !decoded.ok {
        return Err(anyhow!(
            "Telegram {} failed: {}",
            method,
            decoded
                .description
                .unwrap_or_else(|| format!("HTTP {}", status_code))
        ));
    }

    decoded
        .result
        .ok_or_else(|| anyhow!("Telegram {} returned no result", method))
}

#[derive(Debug, Deserialize)]
struct TelegramApiResponse<T> {
    ok: bool,
    result: Option<T>,
    description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TelegramUpdate {
    pub update_id: i64,
    pub message: Option<TelegramMessage>,
    pub my_chat_member: Option<TelegramChatMemberUpdated>,
    pub callback_query: Option<TelegramCallbackQuery>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TelegramMessage {
    pub message_id: i64,
    pub message_thread_id: Option<i64>,
    pub date: Option<u64>,
    pub text: Option<String>,
    pub caption: Option<String>,
    pub from: Option<TelegramUser>,
    pub chat: TelegramChat,
    pub reply_to_message: Option<Box<TelegramMessage>>,
    pub new_chat_members: Option<Vec<TelegramUser>>,
}

impl TelegramMessage {
    pub fn text_content(&self) -> Option<&str> {
        self.text
            .as_deref()
            .or(self.caption.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub fn sender_name(&self) -> String {
        self.from
            .as_ref()
            .map(TelegramUser::display_name)
            .unwrap_or_else(|| "Unknown".to_string())
    }

    pub fn chat_title(&self) -> String {
        self.chat.title.clone().unwrap_or_else(|| {
            self.chat
                .username
                .clone()
                .unwrap_or_else(|| self.chat.id.to_string())
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TelegramChat {
    pub id: i64,
    #[serde(rename = "type")]
    pub kind: String,
    pub title: Option<String>,
    pub username: Option<String>,
    pub is_forum: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TelegramUser {
    pub id: i64,
    pub is_bot: bool,
    pub first_name: String,
    pub last_name: Option<String>,
    pub username: Option<String>,
    pub can_read_all_group_messages: Option<bool>,
}

impl TelegramUser {
    pub fn display_name(&self) -> String {
        match (&self.first_name, &self.last_name) {
            (first, Some(last)) if !last.trim().is_empty() => format!("{first} {last}"),
            (first, _) => first.to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TelegramChatMemberUpdated {
    pub chat: TelegramChat,
    pub new_chat_member: TelegramChatMember,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TelegramChatMember {
    pub user: TelegramUser,
    pub status: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TelegramCallbackQuery {
    pub id: String,
    pub from: TelegramUser,
    pub message: Option<TelegramMessage>,
    pub data: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InlineKeyboardButton {
    pub text: String,
    pub callback_data: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InlineKeyboardMarkup {
    pub inline_keyboard: Vec<Vec<InlineKeyboardButton>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SendMessageRequest {
    pub chat_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_thread_id: Option<i64>,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_message_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disable_notification: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_markup: Option<InlineKeyboardMarkup>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EditMessageTextRequest {
    pub chat_id: i64,
    pub message_id: i64,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_markup: Option<InlineKeyboardMarkup>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnswerCallbackQueryRequest {
    pub callback_query_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_alert: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct TelegramBotIdentity {
    pub id: i64,
    pub username: String,
    pub can_read_all_group_messages: bool,
}

impl TelegramBotIdentity {
    pub fn from_user(user: TelegramUser) -> Result<Self> {
        let username = user
            .username
            .clone()
            .ok_or_else(|| anyhow!("Telegram bot user is missing a username"))?;
        Ok(Self {
            id: user.id,
            username,
            can_read_all_group_messages: user.can_read_all_group_messages.unwrap_or(false),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::parse_response_body;

    #[test]
    fn parses_boolean_result_payloads() {
        let body = r#"{"ok":true,"result":true,"description":"Webhook was set"}"#;
        let result: bool = parse_response_body("setWebhook", 200, true, body).unwrap();
        assert!(result);
    }
}
