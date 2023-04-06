use edge_gpt::{ChatSession, ConversationStyle, CookieInFile};
use ezio::prelude::*;
use isahc::AsyncReadResponseExt;
use libaes::Cipher;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::env;

fn decrypt(data: &[u8], secret: &[u8]) -> String {
    let key = &secret[0..32];
    let iv = &secret[32..(32 + 16)];
    let cipher = Cipher::new_256(key.try_into().unwrap());
    String::from_utf8(cipher.cbc_decrypt(iv, data)).unwrap()
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
}

impl Default for Response {
    fn default() -> Self {
        Self {
            chat_id: Default::default(),
            reply_to_message_id: Default::default(),
            text: Default::default(),
            parse_mode: "Markdown".to_string(),
        }
    }
}

impl Response {
    pub fn new(chat_id: i64, reply_to_message_id: i64, answer: String) -> Self {
        Self {
            chat_id,
            reply_to_message_id,
            text: answer,
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
            ChatSession::create(style, &cookies).await
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
        ChatSession::create(style, &cookies).await
    };
    let response = chat_session.send_message(&request.question).await.unwrap();
    let telegram_response = Response::new(request.chat_id, request.message_id, response.text);
    let response_json = serde_json::to_string(&telegram_response).unwrap();
    let url = format!("https://api.telegram.org/bot{telegram_token}/sendMessage");
    let session_str = serde_json::to_string(&chat_session).unwrap();
    let send_message_request = isahc::Request::builder()
        .uri(url)
        .header("Content-Type", "application/json")
        .method("POST")
        .body(response_json)
        .unwrap();
    let mut send_message_response = isahc::send_async(send_message_request).await.unwrap();
    let send_message_response: SendMessageResponse = send_message_response.json().await.unwrap();
    let key = format!(
        "{}-{}",
        request.chat_id, send_message_response.result.message_id
    );
    let _: () = redis_connection
        .set_ex(key, session_str, 60 * 60)
        .await
        .unwrap();
}
