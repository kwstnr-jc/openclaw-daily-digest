#!/usr/bin/env bash
set -euo pipefail
TOKEN_FILE="/Users/agent/.digest-bot-token"
CHANNEL_ID="1477340656350396668"
API="https://discord.com/api/v10"

post_digest() {
  local body="$1"
  if [[ ! -f "$TOKEN_FILE" ]]; then
    echo "DISCORD: No token file at $TOKEN_FILE. Payload:"
    echo "$body"
    return 0
  fi
  local token
  token=$(tr -d '[:space:]' < "$TOKEN_FILE")
  local json
  json=$(jq -n --arg content "$body" '{content: $content}')
  local code
  code=$(curl -s -o /dev/null -w "%{http_code}" -X POST \
    "$API/channels/$CHANNEL_ID/messages" \
    -H "Authorization: Bot $token" \
    -H "Content-Type: application/json" \
    -d "$json")
  if [[ "$code" == "200" ]]; then
    echo "DISCORD: Posted successfully (HTTP $code)"
  else
    echo "DISCORD: Failed (HTTP $code)"
  fi
}
