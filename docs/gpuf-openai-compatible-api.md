# GPUFabric OpenAI-Compatible API Current Status

Last updated: 2026-07-02

This document describes the current external inference API surface for
GPUFabric compute sharing. It is based on the current `gpuf-s` inference
gateway and `gpuf-c` standalone llama server implementation.

## Scope

There are two related API surfaces:

| Surface | Purpose | Current compatibility |
|---|---|---|
| `gpuf-s` inference gateway | Public compute-sharing entrypoint. It authenticates API tokens, selects an online `gpuf-c`, forwards inference tasks, and records token usage. | OpenAI-compatible Chat Completions, legacy Completions, and Models only |
| `gpuf-c` standalone llama server | Local standalone inference server, not the shared gateway path. | OpenAI-compatible endpoints plus a local Anthropic-compatible `/v1/messages` route |

For external compute-sharing integrations, use the `gpuf-s` inference gateway.

Base URL:

```text
http://<gpuf-s-host>:<inference_gateway_port>
```

The default in-process inference gateway port is `8081`. Test deployments may
map it to another host port, such as `18182`.

## Authentication

All `gpuf-s` inference gateway routes require a bearer token:

```http
Authorization: Bearer <token>
Content-Type: application/json
```

The token is resolved by `gpuf-s` to:

- allowed `client_id` values
- access level
- token hash used for internal token usage statistics

If the header is missing or invalid, the gateway returns `401`.

## Supported Endpoints On gpuf-s

| Endpoint | Status | Notes |
|---|---|---|
| `POST /v1/chat/completions` | Supported | Main recommended endpoint |
| `POST /v1/completions` | Supported | Legacy text completion endpoint |
| `GET /v1/models` | Supported, simplified | Returns a simple model list, not the full OpenAI list envelope |
| `POST /v1/messages` | Not supported on `gpuf-s` | Anthropic-compatible route exists only in `gpuf-c` standalone llama server |
| `POST /v1/embeddings` | Not supported | No gateway route |
| `POST /v1/responses` | Not supported | No gateway route |
| image/audio/vector/file APIs | Not supported | No gateway route |

## POST `/v1/chat/completions`

Recommended for new integrations.

### Request

```json
{
  "model": "gpuf",
  "messages": [
    {
      "role": "user",
      "content": "Hello"
    }
  ],
  "max_tokens": 1024,
  "temperature": 0.7,
  "top_k": 40,
  "top_p": 0.9,
  "repeat_penalty": 1.1,
  "repeat_last_n": 64,
  "min_keep": 1,
  "stream": false,
  "session_id": "optional-session-id",
  "cache_policy": "auto"
}
```

### Supported Fields

| Field | Type | Required | Default | Notes |
|---|---:|---:|---:|---|
| `model` | string | No | `gpuf` | Used by routing/model matching when devices report loaded models |
| `messages` | array | Yes | none | Each message must be `{role: string, content: string}` |
| `max_tokens` | integer | No | `4090` for request dispatch, `1024` for finish-reason comparison in non-stream fallback | Sent to `gpuf-c` as generation limit |
| `temperature` | number | No | `0.7` | Sampling |
| `top_k` | integer | No | `40` | Sampling |
| `top_p` | number | No | `0.9` | Sampling |
| `repeat_penalty` | number | No | `1.1` | Sampling |
| `repeat_last_n` | integer | No | `64` | Sampling |
| `min_keep` | integer | No | `1` | Sampling |
| `stream` | boolean | No | `false` | When `true`, returns SSE chunks |
| `session_id` | string | No | none | GPUFabric extension for sticky routing |
| `cache_policy` | string | No | `auto` | GPUFabric extension: `auto`, `bypass`, or `reset`; requires `session_id` unless `auto` |

### Message Format Limitations

Currently supported:

```json
{"role": "user", "content": "plain text"}
```

Currently not supported by the gateway request schema:

- OpenAI multimodal `content` arrays
- image URL or base64 image blocks
- tool/function call message content
- structured content parts

### Non-Streaming Response

```json
{
  "id": "<task-id>",
  "object": "chat.completion",
  "created": 1710000000,
  "model": "gpuf",
  "session_id": "optional-session-id",
  "client_id": "<selected-client-id>",
  "cache_status": "cold",
  "choices": [
    {
      "index": 0,
      "message": {
        "role": "assistant",
        "content": "Hello, how can I help?"
      },
      "finish_reason": "stop"
    }
  ],
  "usage": {
    "prompt_tokens": 10,
    "completion_tokens": 20,
    "total_tokens": 30,
    "analysis_tokens": 0,
    "final_tokens": 20
  }
}
```

GPUFabric adds optional fields that are not part of the standard OpenAI schema:

- `session_id`
- `client_id`
- `cache_status`
- `analysis_tokens`
- `final_tokens`

### Streaming Response

Set:

```json
{"stream": true}
```

The response is Server-Sent Events:

```text
data: {"id":"<task-id>","object":"chat.completion.chunk","created":1710000000,"model":"gpuf","choices":[{"index":0,"delta":{"role":"assistant","content":"Hel"},"finish_reason":null}]}

data: {"id":"<task-id>","object":"chat.completion.chunk","created":1710000000,"model":"gpuf","choices":[{"index":0,"delta":{"role":"assistant","content":"lo"},"finish_reason":null}]}

data: {"id":"<task-id>","object":"chat.completion.chunk","created":1710000000,"model":"gpuf","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":20,"total_tokens":30,"analysis_tokens":0,"final_tokens":20}}

data: [DONE]
```

For models that produce a separate analysis/thinking phase, a stream delta may
use:

```json
{"role": "assistant", "reasoning_content": "..."}
```

This is a GPUFabric extension and not standard OpenAI Chat Completions output.

### Example

```bash
curl -sS http://<gpuf-s-host>:<inference_gateway_port>/v1/chat/completions \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpuf",
    "messages": [{"role": "user", "content": "Reply only: OK"}],
    "max_tokens": 8,
    "temperature": 0.1,
    "stream": false
  }'
```

## POST `/v1/completions`

Legacy text completion endpoint.

### Request

```json
{
  "model": "gpuf",
  "prompt": "Reply only: OK",
  "max_tokens": 8,
  "temperature": 0.1,
  "top_k": 40,
  "top_p": 0.9,
  "repeat_penalty": 1.1,
  "repeat_last_n": 64,
  "min_keep": 1,
  "stream": false,
  "session_id": "optional-session-id",
  "cache_policy": "auto"
}
```

### Supported Fields

| Field | Type | Required | Default | Notes |
|---|---:|---:|---:|---|
| `prompt` | string | Yes | none | Plain text prompt |
| `model` | string | No | `gpuf` | Used for routing/model matching |
| `max_tokens` | integer | No | `4090` for stream dispatch, `1024` for non-stream dispatch | Generation limit |
| `temperature` | number | No | `0.7` | Sampling |
| `top_k` | integer | No | `40` | Sampling |
| `top_p` | number | No | `0.9` | Sampling |
| `repeat_penalty` | number | No | `1.1` | Sampling |
| `repeat_last_n` | integer | No | `64` | Sampling |
| `min_keep` | integer | No | `1` | Sampling |
| `stream` | boolean | No | `false` | When `true`, returns SSE chunks |
| `session_id` | string | No | none | GPUFabric extension for sticky routing |
| `cache_policy` | string | No | `auto` | GPUFabric extension: `auto`, `bypass`, or `reset`; requires `session_id` unless `auto` |

### Non-Streaming Response

```json
{
  "id": "<task-id>",
  "object": "text_completion",
  "created": 1710000000,
  "model": "gpuf-android",
  "session_id": "optional-session-id",
  "client_id": "<selected-client-id>",
  "cache_status": "cold",
  "choices": [
    {
      "text": "OK",
      "index": 0,
      "logprobs": null,
      "finish_reason": "stop"
    }
  ],
  "usage": {
    "prompt_tokens": 5,
    "completion_tokens": 1,
    "total_tokens": 6,
    "analysis_tokens": null,
    "final_tokens": null
  }
}
```

### Streaming Response

```text
data: {"id":"<task-id>","object":"text_completion","created":1710000000,"model":"gpuf","choices":[{"index":0,"text":"O","finish_reason":null}]}

data: {"id":"<task-id>","object":"text_completion","created":1710000000,"model":"gpuf","choices":[{"index":0,"text":"K","finish_reason":null}]}

data: {"id":"<task-id>","object":"text_completion","created":1710000000,"model":"gpuf","choices":[{"index":0,"text":"","finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":1,"total_tokens":6,"analysis_tokens":null,"final_tokens":null}}

data: [DONE]
```

### Example

```bash
curl -sS http://<gpuf-s-host>:<inference_gateway_port>/v1/completions \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpuf",
    "prompt": "Reply only: OK",
    "max_tokens": 8,
    "temperature": 0.1,
    "stream": false
  }'
```

## GET `/v1/models`

Returns the currently hard-coded simplified gateway model list:

```json
[
  {
    "id": "gpuf-android",
    "object": "model",
    "created": 1710000000,
    "owned_by": "gpuf"
  }
]
```

Important compatibility note: this is not the full OpenAI response shape.
OpenAI normally returns an object containing `object: "list"` and `data: [...]`.
The current gateway returns a bare JSON array.

## GPUFabric Extension Headers

| Header | Direction | Notes |
|---|---|---|
| `request-id` | request | Optional. Used for metered request metrics and token usage correlation |
| `x-target-client-id` | request | Optional. Selects a specific online device. Not allowed for metered tokens; the target must be in the token's allowed client list |
| `x-gpuf-session-id` | request and response | Optional sticky routing session id. Must match body `session_id` if both are provided |
| `x-gpuf-cache-policy` | request | Optional sticky route policy: `auto`, `bypass`, or `reset`. Must match body `cache_policy` if both are provided |
| `x-gpuf-client-id` | response | Selected device/client id |
| `x-gpuf-cache-status` | response | `cold`, `hit`, `bypass`, `reset`, or `evicted` when session routing is active |

## Sticky Session Routing

Sticky routing is a GPUFabric extension that keeps related requests on the same
device when possible.

Use either header fields:

```http
x-gpuf-session-id: session-001
x-gpuf-cache-policy: auto
```

or body fields:

```json
{
  "session_id": "session-001",
  "cache_policy": "auto"
}
```

Rules:

- `cache_policy` can be `auto`, `bypass`, or `reset`.
- If `cache_policy` is not `auto`, `session_id` is required.
- If both header and body values are present, they must match.
- Session routing is scoped by bearer token hash.
- A route can be denied if the previously selected device is no longer allowed
  by the current token.

## Routing And Device Selection

The gateway selects an online authenticated `gpuf-c` device from the token's
allowed client list.

Selection behavior:

- If `x-target-client-id` is provided, the gateway tries to use that device.
- If `model` is provided and devices have reported loaded models, the gateway
  can select a compatible device for that model.
- Otherwise, the gateway picks a low-load authenticated online device.
- If no eligible device is available, the gateway returns `503`.

## Token Usage Statistics

For successful requests with non-zero usage, `gpuf-s` writes a row to
`inference_token_usage`.

Recorded fields include:

- `request_id`
- bearer token hash
- selected `client_id`
- `model`
- endpoint name: `completion` or `chat.completion`
- `prompt_tokens`
- `completion_tokens`
- `total_tokens`
- `stream`
- `created_at`

These rows drive the banking/admin token throughput APIs and compute-map token
summary fields.

## Error Format

The gateway generally returns OpenAI-style error JSON:

```json
{
  "error": {
    "message": "No available Android devices found",
    "type": "api_error",
    "code": 503
  }
}
```

Common statuses:

| Status | Typical reason |
|---:|---|
| `400` | Invalid request headers, invalid `x-target-client-id`, invalid session/cache fields |
| `401` | Missing or invalid bearer token |
| `403` | Target client forbidden or session route no longer allowed |
| `503` | No eligible online device |
| `500` | Internal gateway or device execution error |

## Unsupported OpenAI Features

The current gateway does not implement:

- `/v1/responses`
- `/v1/embeddings`
- `/v1/audio/*`
- `/v1/images/*`
- `/v1/files/*`
- tools/function calling
- JSON mode / `response_format`
- `n` multiple completions
- `stop` sequences
- `logprobs` output
- `presence_penalty` and `frequency_penalty`
- OpenAI multimodal message content arrays
- OpenAI batch APIs

Unknown JSON fields are ignored by the Rust deserializer unless they are needed
by the supported request structs. Callers should not rely on ignored fields
having any effect.

## Anthropic API Status

`gpuf-s` compute-sharing gateway does not currently expose Anthropic Messages:

```http
POST /v1/messages
```

`gpuf-c` standalone llama server does expose a local Anthropic-compatible route:

```http
POST /v1/messages
```

That standalone route supports basic fields:

```json
{
  "model": "llama.cpp",
  "system": "You are helpful.",
  "messages": [
    {"role": "user", "content": "Hello"}
  ],
  "max_tokens": 1024,
  "temperature": 0.7,
  "stream": true,
  "thinking": {
    "type": "enabled",
    "budget_tokens": 1024
  }
}
```

This route is local to `gpuf-c` standalone mode and is not part of the current
shared `gpuf-s` gateway API. To support Anthropic clients through compute
sharing, `gpuf-s` needs a `/v1/messages` adapter that maps Anthropic Messages
requests to the existing chat inference scheduler and maps responses/SSE events
back to Anthropic format.

## Compatibility Recommendation

For external callers today:

- Prefer `POST /v1/chat/completions`.
- Use plain text `messages[].content`.
- Use `stream: true` only if the client can parse SSE.
- Do not depend on unsupported OpenAI fields.
- Treat `client_id`, `session_id`, and `cache_status` as GPUFabric extensions.
- Use the returned `usage` for billing/visibility when present.

For frontend dashboards and admin APIs, use the separate `gpuf-s` management API
documents instead of this inference API document.
