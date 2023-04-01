from http.server import BaseHTTPRequestHandler
from EdgeGPT import Chatbot
import asyncio
import os
import requests


def send_telegram_message(token, chat_id, message):
    url = f"https://api.telegram.org/bot{token}/sendMessage"
    data = {"chat_id": chat_id, "text": message}
    response = requests.post(url, data=data)
    return response.json()


token = os.environ.get("TELEGRAM_TOKEN")
chat_id = "1199598103"


async def ask(question):
    bot = Chatbot(cookiePath='./cookies.json')
    result = await bot.ask(question)
    print(result['item']['messages'][1]['text'])
    return result['item']['messages'][1]['text']

if __name__ == '__main__':
    with open('question.txt', 'r') as f:
        question = f.read()
    message = asyncio.run(ask(question))
    send_telegram_message(token, chat_id, message)
