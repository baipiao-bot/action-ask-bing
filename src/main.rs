use edge_gpt::{ChatSession, ConversationStyle, CookieInFile, NewBingResponseMessage};
use ezio::prelude::*;
use isahc::AsyncReadResponseExt;
use libaes::Cipher;
use redis::AsyncCommands;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{env, mem, time::Duration};
use tokio::{sync::broadcast, time::interval};
fn decrypt(data: &[u8], secret: &[u8]) -> String {
    let key = &secret[0..32];
    let iv = &secret[32..(32 + 16)];
    let cipher = Cipher::new_256(key.try_into().unwrap());
    String::from_utf8(cipher.cbc_decrypt(iv, data)).unwrap()
}

pub fn escape(text: &str) -> String {
    text.replace('\"', "\\\"")
        .replace('{', "\\{")
        .replace('}', "\\}")
        .replace('_', "\\_")
        .replace('.', "\\.")
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    chat_id: i64,
    message_id: i64,
    reply_to_message_id: Option<i64>,
    question: String,
    #[serde(default)]
    style: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    chat_id: i64,
    reply_to_message_id: i64,
    text: String,
    parse_mode: String,
    disable_web_page_preview: bool,
}

impl Default for Response {
    fn default() -> Self {
        Self {
            chat_id: Default::default(),
            reply_to_message_id: Default::default(),
            text: Default::default(),
            parse_mode: "MarkdownV2".to_string(),
            disable_web_page_preview: true,
        }
    }
}

fn sup(mut number: u8) -> String {
    const SUP_CHARACTERS: &str = "⁰¹²³⁴⁵⁶⁷⁸⁹";
    let mut result = String::new();
    let hundred = number / 100;
    if hundred > 0 {
        result.push(SUP_CHARACTERS.chars().nth(hundred as usize).unwrap());
    }
    number %= 100;
    let ten = number / 10;
    if hundred > 0 || ten > 0 {
        result.push(SUP_CHARACTERS.chars().nth(ten as usize).unwrap());
    }
    number %= 10;
    result.push(SUP_CHARACTERS.chars().nth(number as usize).unwrap());
    result
}

impl Response {
    fn fix_attributions(answer: &mut NewBingResponseMessage) {
        let mut text = mem::take(&mut answer.text);
        for (attribution_id, source_attribution) in answer.source_attributions.iter().enumerate() {
            let display_form_attribution_id = attribution_id + 1;
            let display_form_attribution_id_sup_str = sup(display_form_attribution_id as _);
            text = text.replace(
                &format!("[^{display_form_attribution_id}^]"),
                &format!("[{display_form_attribution_id_sup_str}]({source_attribution})"),
            );
        }
        answer.text = text;
    }

    fn fix_unordered_list(answer: &mut NewBingResponseMessage) {
        answer.text.insert(0, '\n');
        let re = Regex::new("^[-]").unwrap();
        answer.text = re.replace(&answer.text, "•").to_string();
        answer.text.remove(0);
    }

    fn parse_answer(mut answer: NewBingResponseMessage) -> String {
        Self::fix_attributions(&mut answer);
        Self::fix_unordered_list(&mut answer);
        escape(&answer.text)
    }

    pub fn new(chat_id: i64, reply_to_message_id: i64, answer: NewBingResponseMessage) -> Self {
        Self {
            chat_id,
            reply_to_message_id,
            text: Self::parse_answer(answer),
            ..Default::default()
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct SendMessageResponseResult {
    message_id: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct SendMessageResponse {
    result: SendMessageResponseResult,
}

#[tokio::main]
async fn main() {
    let secret_str = env::var("SECRET").unwrap();
    let redis_url = env::var("REDIS_URL").unwrap();
    let telegram_token = env::var("TELEGRAM_TOKEN").unwrap();

    let secret = hex::decode(secret_str).unwrap();
    let request_encrypted = file::read("./request.json.encrypted");
    let request_str = decrypt(&hex::decode(request_encrypted).unwrap(), &secret);
    let request: Request = serde_json::from_str(&request_str).unwrap();
    let redis_client = redis::Client::open(redis_url).unwrap();
    let mut redis_connection = redis_client.get_async_connection().await.unwrap();
    let mut chat_session = if let Some(reply_to_message_id) = request.reply_to_message_id {
        let key = format!("{}-{}", request.chat_id, reply_to_message_id);
        let corresponding_session: Result<String, redis::RedisError> =
            redis_connection.get(key).await;
        if let Ok(corresponding_session) = corresponding_session {
            serde_json::from_str(&corresponding_session).unwrap()
        } else {
            let cookie_str = env::var("COOKIE").unwrap();
            let cookies: Vec<CookieInFile> = serde_json::from_str(&cookie_str).unwrap();
            let style = match request.style.as_str() {
                "" | "creative" => ConversationStyle::Creative,
                "balanced" => ConversationStyle::Balanced,
                "precise" => ConversationStyle::Precise,
                _ => panic!("style must be one of: creative, balanced, precise"),
            };
            ChatSession::create(style, &cookies).await.unwrap()
        }
    } else {
        let cookie_str = env::var("COOKIE").unwrap();
        let cookies: Vec<CookieInFile> = serde_json::from_str(&cookie_str).unwrap();
        let style = match request.style.as_str() {
            "" | "creative" => ConversationStyle::Creative,
            "balanced" => ConversationStyle::Balanced,
            "precise" => ConversationStyle::Precise,
            _ => panic!("style must be one of: creative, balanced, precise"),
        };
        ChatSession::create(style, &cookies).await.unwrap()
    };

    let (stop_typing_action_tx, mut stop_typing_action_rx) = broadcast::channel(1);
    let chat_id = request.chat_id;
    let telegram_token_cloned = telegram_token.clone();
    tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(5));
        loop {
            tokio::select! {
                _ = stop_typing_action_rx.recv() => {
                    break;
                }
                _ = interval.tick() => {
                    let send_message_request: isahc::Request<String> = isahc::Request::builder()
                        .uri(format!("https://api.telegram.org/bot{telegram_token_cloned}/sendChatAction"))
                        .header("Content-Type", "application/json")
                        .method("POST")
                        .body(format!("{{\"chat_id\": {chat_id}, \"action\": \"typing\"}}"))
                        .unwrap();
                    isahc::send_async(send_message_request).await.unwrap();
                }
            }
        }
    });

    let response = chat_session.send_message(&request.question).await.unwrap();
    let telegram_response = Response::new(request.chat_id, request.message_id, response);
    let response_json = serde_json::to_string(&telegram_response).unwrap();
    let url = format!("https://api.telegram.org/bot{telegram_token}/sendMessage");
    let session_str = serde_json::to_string(&chat_session).unwrap();
    let send_message_request = isahc::Request::builder()
        .uri(url)
        .header("Content-Type", "application/json")
        .method("POST")
        .body(response_json.clone())
        .unwrap();
    let mut send_message_response = isahc::send_async(send_message_request)
        .await
        .map_err(|_err| panic!("failed to send message: {response_json}"))
        .unwrap();
    stop_typing_action_tx.send(()).unwrap();
    let send_message_response: SendMessageResponse = send_message_response
        .json()
        .await
        .map_err(|_err| panic!("failed to send message: {response_json}"))
        .unwrap();
    let key = format!(
        "{}-{}",
        request.chat_id, send_message_response.result.message_id
    );
    let _: () = redis_connection
        .set_ex(key, session_str, 60 * 60)
        .await
        .unwrap();
}
