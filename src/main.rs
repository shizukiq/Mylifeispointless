mod prompts;

use std::collections::{HashMap, VecDeque};
use std::env;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use reqwest::{Client, Url};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};

const HISTORY_MESSAGES: usize = 12;
const TELEGRAM_BASE: &str = "https://api.telegram.org";
const OPENROUTER_CHAT: &str = "https://openrouter.ai/api/v1/chat/completions";

struct Settings {
    telegram_token: String,
    openrouter_key: String,
    model: String,
    reasoning_effort: String,
    allowed_user_id: Option<i64>,
}

#[derive(Clone, Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct TelegramUpdate {
    update_id: i64,
    message: Option<TelegramMessage>,
}

#[derive(Deserialize)]
struct TelegramMessage {
    message_id: i64,
    text: Option<String>,
    chat: TelegramChat,
    from: Option<TelegramUser>,
}

#[derive(Deserialize)]
struct TelegramChat {
    id: i64,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct TelegramUser {
    id: i64,
}

#[derive(Deserialize)]
struct TelegramResponse<T> {
    ok: bool,
    result: Option<T>,
    description: Option<String>,
}

#[derive(Deserialize)]
struct OpenRouterResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: AssistantMessage,
}

#[derive(Deserialize)]
struct AssistantMessage {
    content: Option<String>,
}

fn load_env_file() {
    let Ok(contents) = std::fs::read_to_string(".env") else {
        return;
    };
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if env::var_os(key).is_none() {
            env::set_var(key, value.trim().trim_matches(['"', '\'']));
        }
    }
}

fn settings() -> Result<Settings> {
    let telegram_token = env::var("TELEGRAM_BOT_TOKEN")
        .context("set TELEGRAM_BOT_TOKEN in .env or the environment")?;
    let openrouter_key = env::var("OPENROUTER_API_KEY")
        .context("set OPENROUTER_API_KEY in .env or the environment")?;
    let allowed_user_id = env::var("ALLOWED_TELEGRAM_USER_ID")
        .ok()
        .filter(|id| !id.trim().is_empty())
        .map(|id| {
            id.parse::<i64>()
                .context("ALLOWED_TELEGRAM_USER_ID must be a number")
        })
        .transpose()?;

    Ok(Settings {
        telegram_token,
        openrouter_key,
        model: env::var("OPENROUTER_MODEL")
            .unwrap_or_else(|_| "deepseek/deepseek-v4.1-flash".to_string()),
        reasoning_effort: env::var("OPENROUTER_REASONING_EFFORT")
            .unwrap_or_else(|_| "none".to_string()),
        allowed_user_id,
    })
}

async fn telegram<T: DeserializeOwned>(
    client: &Client,
    token: &str,
    method: &str,
    body: Value,
) -> Result<T> {
    let endpoint = Url::parse(&format!("{TELEGRAM_BASE}/bot{token}/{method}"))?;
    let response = client
        .post(endpoint)
        .json(&body)
        .send()
        .await
        .map_err(|error| anyhow!("Telegram request failed: {}", error.without_url()))?;
    let payload: TelegramResponse<T> = response
        .json()
        .await
        .context("Telegram returned invalid JSON")?;
    if !payload.ok {
        bail!(
            "Telegram API error: {}",
            payload.description.unwrap_or_default()
        );
    }
    payload
        .result
        .ok_or_else(|| anyhow!("Telegram response did not contain a result"))
}

async fn reply(
    client: &Client,
    settings: &Settings,
    history: &mut HashMap<i64, VecDeque<Message>>,
    message: TelegramMessage,
) -> Result<()> {
    if message.chat.kind != "private"
        || settings
            .allowed_user_id
            .is_some_and(|allowed| message.from.as_ref().map(|user| user.id) != Some(allowed))
    {
        return Ok(());
    }

    let Some(text) = message.text else {
        return Ok(());
    };
    let text = text.trim();
    if text == "/start" {
        telegram::<Value>(
            client,
            &settings.telegram_token,
            "sendMessage",
            json!({"chat_id": message.chat.id, "text": "Можешь просто написать, что происходит. Если нужен совет — скажи прямо."}),
        )
        .await?;
        return Ok(());
    }
    if text == "/reset" {
        history.remove(&message.chat.id);
        telegram::<Value>(
            client,
            &settings.telegram_token,
            "sendMessage",
            json!({"chat_id": message.chat.id, "text": "Окей, начал разговор заново."}),
        )
        .await?;
        return Ok(());
    }
    if text.is_empty() {
        return Ok(());
    }

    let conversation = history.entry(message.chat.id).or_default();
    conversation.push_back(Message {
        role: "user".to_string(),
        content: text.to_string(),
    });
    while conversation.len() > HISTORY_MESSAGES {
        conversation.pop_front();
    }

    let mut messages = Vec::with_capacity(conversation.len() + 1);
    messages.push(json!({"role": "system", "content": prompts::SYSTEM}));
    messages.extend(
        conversation
            .iter()
            .map(|message| json!({"role": message.role, "content": message.content})),
    );
    let response = client
        .post(OPENROUTER_CHAT)
        .bearer_auth(&settings.openrouter_key)
        .json(&json!({
            "model": settings.model,
            "messages": messages,
            "reasoning": {"effort": settings.reasoning_effort},
            "max_tokens": 500
        }))
        .send()
        .await
        .map_err(|error| anyhow!("OpenRouter request failed: {}", error.without_url()))?;
    let result: OpenRouterResponse = response
        .json()
        .await
        .context("OpenRouter returned invalid JSON")?;
    let answer = result
        .choices
        .into_iter()
        .next()
        .and_then(|choice| choice.message.content)
        .filter(|answer| !answer.trim().is_empty())
        .ok_or_else(|| anyhow!("OpenRouter returned an empty answer"))?;

    conversation.push_back(Message {
        role: "assistant".to_string(),
        content: answer.clone(),
    });
    while conversation.len() > HISTORY_MESSAGES {
        conversation.pop_front();
    }
    let answer: String = answer.chars().take(3900).collect();

    telegram::<Value>(
        client,
        &settings.telegram_token,
        "sendMessage",
        json!({
            "chat_id": message.chat.id,
            "text": answer,
            "reply_parameters": {"message_id": message.message_id}
        }),
    )
    .await?;
    Ok(())
}

async fn check_prompts(client: &Client, settings: &Settings) -> Result<()> {
    for case in prompts::CASES {
        let response = client
            .post(OPENROUTER_CHAT)
            .bearer_auth(&settings.openrouter_key)
            .json(&json!({
                "model": settings.model,
                "messages": [
                    {"role": "system", "content": prompts::SYSTEM},
                    {"role": "user", "content": case.message}
                ],
                "reasoning": {"effort": settings.reasoning_effort},
                "max_tokens": 500
            }))
            .send()
            .await
            .map_err(|error| anyhow!("OpenRouter request failed: {}", error.without_url()))?;
        let result: OpenRouterResponse = response
            .json()
            .await
            .with_context(|| format!("invalid OpenRouter response for {}", case.name))?;
        let answer = result
            .choices
            .into_iter()
            .next()
            .and_then(|choice| choice.message.content)
            .unwrap_or_else(|| "[empty response]".to_string());
        println!("--- {} ---\n{}\n", case.name, answer);
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    load_env_file();
    let settings = settings()?;
    let client = Client::builder()
        .timeout(Duration::from_secs(45))
        .build()
        .context("could not create HTTP client")?;

    if env::args().any(|arg| arg == "--prompt-check") {
        return check_prompts(&client, &settings).await;
    }

    let mut history = HashMap::<i64, VecDeque<Message>>::new();
    let mut offset = 0_i64;
    loop {
        let updates: Vec<TelegramUpdate> = telegram(
            &client,
            &settings.telegram_token,
            "getUpdates",
            json!({"offset": offset, "timeout": 30, "allowed_updates": ["message"]}),
        )
        .await?;

        for update in updates {
            offset = update.update_id + 1;
            if let Some(message) = update.message {
                if let Err(error) = reply(&client, &settings, &mut history, message).await {
                    eprintln!("message handling failed: {error:#}");
                }
            }
        }
    }
}
