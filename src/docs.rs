use axum::{response::{Html, IntoResponse}, Json};

pub async fn docs_handler() -> impl IntoResponse {
    Html(r#"<!doctype html>
<html lang="en">
  <head>
    <title>VoltGate API Reference</title>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <style>
      body {
        margin: 0;
      }
    </style>
  </head>
  <body>
    <script
      id="api-reference"
      data-url="/openapi.json"
      data-configuration='{ "theme": "dark", "layout": "sidebar" }'></script>
    <script src="https://cdn.jsdelivr.net/npm/@scalar/api-reference"></script>
  </body>
</html>"#)
}

pub async fn openapi_json_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "openapi": "3.0.3",
        "info": {
            "title": "VoltGate API",
            "version": "0.1.0",
            "description": "VoltGate — Open-source LLM router proxy for Anthropic models. Fully compatible with OpenAI's Chat Completions and Embeddings client SDKs."
        },
        "servers": [
            {
                "url": "http://localhost:3001",
                "description": "Local VoltGate instance"
            }
        ],
        "paths": {
            "/v1/chat/completions": {
                "post": {
                    "summary": "Create chat completion",
                    "description": "Proxy endpoint that routes requests to the cheapest reliable Anthropic model after executing classification, rate limits, and context engineering.",
                    "security": [
                        {
                            "BearerAuth": []
                        }
                    ],
                    "parameters": [
                        {
                            "name": "x-caller-id",
                            "in": "header",
                            "description": "Identifier for the caller (used for RPM rate limits and hour-spend tracking)",
                            "required": false,
                            "schema": {
                                "type": "string",
                                "default": "anonymous"
                            }
                        }
                    ],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "$ref": "#/components/schemas/ChatCompletionRequest"
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "Successful completion response",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "$ref": "#/components/schemas/ChatCompletionResponse"
                                    }
                                }
                            }
                        },
                        "400": {
                            "description": "Bad Request (e.g. guardrail violation)"
                        },
                        "429": {
                            "description": "Too Many Requests (rate limit or daily model budget exceeded)"
                        }
                    }
                }
            },
            "/v1/embeddings": {
                "post": {
                    "summary": "Create text embeddings",
                    "description": "Exposes an OpenAI-compatible text embeddings endpoint forwarding to Voyage AI or local deterministic mock embeddings.",
                    "security": [
                        {
                            "BearerAuth": []
                        }
                    ],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "$ref": "#/components/schemas/EmbeddingsRequest"
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "Successful embeddings response",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "$ref": "#/components/schemas/EmbeddingsResponse"
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "/health": {
                "get": {
                    "summary": "Service health check",
                    "responses": {
                        "200": {
                            "description": "Service is healthy"
                        }
                    }
                }
            }
        },
        "components": {
            "securitySchemes": {
                "BearerAuth": {
                    "type": "http",
                    "scheme": "bearer",
                    "description": "Submit the ROUTER_API_KEY as the bearer token to authenticate requests."
                }
            },
            "schemas": {
                "ChatCompletionRequest": {
                    "type": "object",
                    "required": ["messages"],
                    "properties": {
                        "model": {
                            "type": "string",
                            "description": "Optional model target. If omitted, VoltGate will classify and auto-route.",
                            "example": "claude-sonnet-4-6"
                        },
                        "messages": {
                            "type": "array",
                            "items": {
                                "$ref": "#/components/schemas/ChatMessage"
                            }
                        },
                        "max_tokens": {
                            "type": "integer",
                            "default": 1024
                        },
                        "temperature": {
                            "type": "number",
                            "default": 0.7
                        },
                        "stream": {
                            "type": "boolean",
                            "default": false
                        }
                    }
                },
                "ChatMessage": {
                    "type": "object",
                    "required": ["role", "content"],
                    "properties": {
                        "role": {
                            "type": "string",
                            "enum": ["system", "user", "assistant"],
                            "example": "user"
                        },
                        "content": {
                            "type": "string",
                            "example": "Hello, how do I compute dynamic programming edit distance?"
                        }
                    }
                },
                "ChatCompletionResponse": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "object": { "type": "string", "example": "chat.completion" },
                        "created": { "type": "integer" },
                        "model": { "type": "string", "example": "claude-sonnet-4-6" },
                        "choices": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "index": { "type": "integer" },
                                    "message": {
                                        "$ref": "#/components/schemas/ChatMessage"
                                    },
                                    "finish_reason": { "type": "string", "example": "end_turn" }
                                }
                            }
                        },
                        "usage": {
                            "type": "object",
                            "properties": {
                                "prompt_tokens": { "type": "integer" },
                                "completion_tokens": { "type": "integer" },
                                "total_tokens": { "type": "integer" }
                            }
                        },
                        "x_router": {
                            "type": "object",
                            "description": "Custom VoltGate metadata tracing the routing execution path",
                            "properties": {
                                "routed_to": { "type": "string" },
                                "complexity": { "type": "string" },
                                "task_type": { "type": "string" },
                                "cost_usd": { "type": "number" },
                                "cache_hit": { "type": "boolean" },
                                "reasoning": { "type": "string" }
                            }
                        }
                    }
                },
                "EmbeddingsRequest": {
                    "type": "object",
                    "required": ["input"],
                    "properties": {
                        "input": {
                            "oneOf": [
                                { "type": "string" },
                                { "type": "array", "items": { "type": "string" } }
                            ],
                            "example": "VoltGate LLM router embeddings input text"
                        },
                        "model": {
                            "type": "string",
                            "default": "text-embedding-3-small"
                        }
                    }
                },
                "EmbeddingsResponse": {
                    "type": "object",
                    "properties": {
                        "object": { "type": "string", "example": "list" },
                        "model": { "type": "string", "example": "text-embedding-3-small" },
                        "data": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "object": { "type": "string", "example": "embedding" },
                                    "embedding": {
                                        "type": "array",
                                        "items": { "type": "number" }
                                    },
                                    "index": { "type": "integer" }
                                }
                            }
                        },
                        "usage": {
                            "type": "object",
                            "properties": {
                                "prompt_tokens": { "type": "integer" },
                                "total_tokens": { "type": "integer" }
                            }
                        }
                    }
                }
            }
        }
    }))
}
