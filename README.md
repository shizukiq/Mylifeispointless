# prosto_bot

A small Telegram Bot API bot for one-to-one conversations. It uses an OpenRouter chat model and keeps a short, in-memory conversation history for each chat.

The bot is meant to be a plain, low-pressure conversation partner: it can listen when someone needs to vent, or offer a couple of practical next steps when they ask what to do. It is not a therapist and does not diagnose people.

## Requirements

- Rust and Cargo
- A Telegram bot token from [@BotFather](https://t.me/BotFather)
- An OpenRouter API key

## Setup

Copy the example environment file and add your credentials:

```sh
cp .env.example .env
```

Set `TELEGRAM_BOT_TOKEN` and `OPENROUTER_API_KEY` in `.env`, then start the bot:

```sh
cargo run
```

The bot uses long polling, so it does not need a public server or webhook. It handles private text messages only.

## Configuration

| Variable | Required | Default | Purpose |
| --- | --- | --- | --- |
| `TELEGRAM_BOT_TOKEN` | Yes | — | Token issued by BotFather. |
| `OPENROUTER_API_KEY` | Yes | — | Key used to call OpenRouter. |
| `OPENROUTER_MODEL` | No | `deepseek/deepseek-v4.1-flash` | OpenRouter model ID. |
| `OPENROUTER_REASONING_EFFORT` | No | `none` | Reasoning effort sent to OpenRouter. |
| `ALLOWED_TELEGRAM_USER_ID` | No | — | If set, only this numeric Telegram user ID can use the bot. |

Environment variables already present in the shell take precedence over values in `.env`.

## Commands

- `/start` — show a short introduction.
- `/reset` — clear the current chat's in-memory conversation history.

The bot retains up to 12 recent messages per chat in memory. This history is lost when the process exits and is not written to disk.

## Prompt check

Run four sample conversations through the configured model and print the replies:

```sh
cargo run -- --prompt-check
```

The cases cover venting without advice, a direct request for advice, an unclear support request, and immediate danger. Replies are shown for manual review; the command does not score them or guarantee response quality. This mode needs `OPENROUTER_API_KEY` and uses the configured model, but does not contact Telegram.

## Scope and safety

The system prompt asks the model to keep ordinary replies brief, avoid canned reassurance, and not suggest professional or hotline services by default. It also tells the model to respond more directly if someone describes immediate danger. Model output is not a substitute for professional care or emergency services.
