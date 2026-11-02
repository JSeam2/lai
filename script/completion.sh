#!/bin/sh
set -eu

if [ "$#" -ne 1 ] || [ -z "$1" ]; then
    echo 'Usage: ./completion.sh "message"' >&2
    exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
    if ! command -v brew >/dev/null 2>&1; then
        echo 'jq is required, but Homebrew is not installed or not on PATH. Install Homebrew from https://brew.sh and try again.' >&2
        exit 1
    fi
    echo 'jq is not installed. Installing it with Homebrew...' >&2
    if ! brew install jq >&2; then
        echo 'Could not install jq. Run brew install jq to diagnose the problem, then try again.' >&2
        exit 1
    fi
    if ! command -v jq >/dev/null 2>&1; then
        echo 'Homebrew installed jq, but it is not on PATH. Add your Homebrew bin directory to PATH and try again.' >&2
        exit 1
    fi
fi

base_url=${LAI_BASE_URL:-http://127.0.0.1:8989/v1}
base_url=${base_url%/}

if ! models=$(curl --fail --silent --show-error "$base_url/models"); then
    echo "Could not reach $base_url. Start the server with 'lai serve'." >&2
    exit 1
fi
if ! model=$(printf '%s' "$models" | jq -er '.data[0].id | strings | select(length > 0)'); then
    echo 'The server did not report a loaded model. Check the server logs.' >&2
    exit 1
fi

payload=$(jq -n --arg model "$model" --arg message "$1" \
    '{model: $model, messages: [{role: "user", content: $message}]}')
if ! response=$(curl --fail --silent --show-error \
    -H 'Content-Type: application/json' --data "$payload" "$base_url/chat/completions"); then
    echo 'The completion request failed. Check the server logs.' >&2
    exit 1
fi
if ! printf '%s' "$response" | jq -er '.choices[0].message.content | strings'; then
    echo 'The server response did not contain a chat reply. Check the server logs.' >&2
    exit 1
fi
