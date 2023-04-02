from http.server import BaseHTTPRequestHandler
import json
from EdgeGPT import Chatbot
import asyncio
import os
import requests

token = os.environ.get("TELEGRAM_TOKEN")


def send_telegram_message(token, chat_id, message):
    url = f"https://api.telegram.org/bot{token}/sendMessage"
    data = {"chat_id": chat_id, "text": message, "parse_mode": "Markdown"}
    response = requests.post(url, data=data)
    return response.json()


async def ask(question):
    bot = Chatbot(cookiePath='./cookies.json')
    result = await bot.ask(question)
    return result['item']['messages'][1]['text']

if __name__ == '__main__':
    with open('question.json', 'r') as f:
        data = json.load(f)
        question = data['question']
        chat_id = data['chat_id']

    message = asyncio.run(ask(question))
    print(send_telegram_message(token, chat_id, message))
