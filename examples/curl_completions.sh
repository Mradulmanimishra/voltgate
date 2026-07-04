#!/bin/bash

# VoltGate Curl Request Example
# To run this script:
#   1. Ensure VoltGate is running on localhost:3001
#   2. Run the script: ./curl_completions.sh

# Resolve API Key from env if present
API_KEY="${ROUTER_API_KEY:-default-open-key}"

echo "⚡ Sending request to VoltGate on port 3001..."
echo ""

curl -X POST http://localhost:3001/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -H "x-caller-id: dev-curl-client" \
  -d '{
    "model": "claude-sonnet-4-6",
    "messages": [
      {
        "role": "user",
        "content": "Hello! What model are you and how did VoltGate route me here?"
      }
    ],
    "max_tokens": 256,
    "temperature": 0.7,
    "stream": false
  }'

echo ""
echo ""
echo "✓ Request complete. Check the x_router metadata field in the JSON response to see routing complexity!"
