from http.server import BaseHTTPRequestHandler
from EdgeGPT import Chatbot
import asyncio


async def ask(question):
    bot = Chatbot(cookiePath='./cookies.json')
    result = await bot.ask(question)
    print(result['item']['messages'][1]['text'])
    return result['item']['messages'][1]['text']

if __name__ == '__main__':
    with open('question.txt', 'r') as f:
        question = f.read()
    print(asyncio.run(ask(question)))
