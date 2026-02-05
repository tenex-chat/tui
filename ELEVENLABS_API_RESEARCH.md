# ElevenLabs Custom LLM Endpoint API Specifications - Comprehensive Research

## Executive Summary

ElevenLabs provides a comprehensive **Agents Platform** that supports integration with custom LLM endpoints. The platform requires custom LLM servers to implement **OpenAI-compatible** `/v1/chat/completions` endpoints. This document provides detailed specifications for implementing a custom LLM backend for ElevenLabs agents.

---

## 1. ElevenLabs Custom LLM API Specification

### 1.1 HTTP Methods and Endpoint Structure

**Required Endpoint Pattern:**
```
POST {BASE_URL}/v1/chat/completions
```

**HTTP Method:** `POST`

**Authentication:** Custom (configured via ElevenLabs dashboard)

**Content-Type:** `application/json`

### 1.2 Base URL Requirements

The custom LLM endpoint must:
- Be publicly accessible (resolvable from ElevenLabs infrastructure)
- Support HTTPS (recommended)
- Be specified during agent configuration
- Can be exposed via ngrok or other tunneling solutions during development

**Example Base URLs:**
```
https://api.yourdomain.com
https://your-app.ngrok.io
https://custom-llm-service.example.com
```

### 1.3 Official Documentation

Primary References:
- ElevenLabs Custom LLM Integration Guide: https://elevenlabs.io/docs/agents-platform/customization/llm/custom-llm
- ElevenLabs API Reference: https://elevenlabs.io/docs/api-reference/introduction
- ElevenLabs GitHub Docs: https://github.com/elevenlabs/elevenlabs-docs

---

## 2. OpenAI /v1/chat/completions Specification (ElevenLabs Requirement)

### 2.1 Request Payload Structure

The custom LLM endpoint must accept requests in this exact format:

```json
{
  "model": "your-model-id",
  "messages": [
    {
      "role": "system",
      "content": "You are a helpful assistant."
    },
    {
      "role": "user",
      "content": "Hello, how can you help?"
    }
  ],
  "temperature": 0.7,
  "top_p": 1.0,
  "max_tokens": 2000,
  "stream": false,
  "tools": [
    {
      "type": "function",
      "function": {
        "name": "tool_name",
        "description": "Tool description",
        "parameters": {
          "type": "object",
          "properties": {
            "param1": {
              "type": "string",
              "description": "Parameter description"
            }
          },
          "required": ["param1"]
        }
      }
    }
  ],
  "tool_choice": "auto"
}
```

### 2.2 Request Field Specifications

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `model` | string | Yes | Model identifier (ElevenLabs specifies this during configuration) |
| `messages` | array | Yes | Array of message objects with conversation history |
| `messages[].role` | string | Yes | Message role: "system", "user", or "assistant" |
| `messages[].content` | string or array | Yes | Message content (text or multi-modal) |
| `temperature` | number | No | Sampling temperature (0-2), controls randomness |
| `top_p` | number | No | Nucleus sampling parameter (0-1) |
| `max_tokens` | number | No | Maximum tokens in response |
| `stream` | boolean | No | Enable streaming response (default: false) |
| `tools` | array | No | List of available functions/tools the model can call |
| `tool_choice` | string | No | How to handle tools: "auto", "required", or specific function name |
| `presence_penalty` | number | No | Penalize new tokens based on presence (-2 to 2) |
| `frequency_penalty` | number | No | Penalize new tokens based on frequency (-2 to 2) |
| `logit_bias` | object | No | Bias specific token log probabilities |

### 2.3 Non-Streaming Response Format

```json
{
  "id": "chatcmpl-123abc",
  "object": "chat.completion",
  "created": 1677649420,
  "model": "your-model-id",
  "choices": [
    {
      "index": 0,
      "message": {
        "role": "assistant",
        "content": "This is the assistant's response."
      },
      "finish_reason": "stop"
    }
  ],
  "usage": {
    "prompt_tokens": 10,
    "completion_tokens": 15,
    "total_tokens": 25
  }
}
```

**Response Fields:**
- `id`: Unique completion identifier
- `object`: Always "chat.completion"
- `created`: Unix timestamp of generation
- `model`: Model used for completion
- `choices`: Array of completion choices
- `choices[].message`: Generated message with role and content
- `choices[].finish_reason`: Why generation stopped ("stop", "length", "tool_calls", etc.)
- `usage`: Token usage statistics

### 2.4 Streaming Response Format (Server-Sent Events)

When `stream: true`, the endpoint should return SSE (Server-Sent Events) format:

```
data: {"id":"chatcmpl-123","object":"chat.completion.chunk","created":1677649420,"model":"your-model-id","choices":[{"index":0,"delta":{"role":"assistant","content":""},"finish_reason":null}]}

data: {"id":"chatcmpl-123","object":"chat.completion.chunk","created":1677649420,"model":"your-model-id","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}

data: {"id":"chatcmpl-123","object":"chat.completion.chunk","created":1677649420,"model":"your-model-id","choices":[{"index":0,"delta":{"content":" there"},"finish_reason":null}]}

data: {"id":"chatcmpl-123","object":"chat.completion.chunk","created":1677649420,"model":"your-model-id","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}

data: [DONE]
```

**Streaming Details:**
- Each chunk must have `object: "chat.completion.chunk"`
- `delta` field contains incremental changes (not full message)
- `finish_reason` is null for all chunks except the final one
- Final chunk has empty `choices` array or `finish_reason: "stop"`
- Terminate stream with `data: [DONE]`
- Content-Type: `text/event-stream`
- Use chunked transfer encoding for HTTP/1.1

### 2.5 Tool/Function Calling in Responses

When the model chooses to call a tool:

```json
{
  "id": "chatcmpl-123abc",
  "object": "chat.completion",
  "created": 1677649420,
  "model": "your-model-id",
  "choices": [
    {
      "index": 0,
      "message": {
        "role": "assistant",
        "content": null,
        "tool_calls": [
          {
            "id": "call_abc123",
            "type": "function",
            "function": {
              "name": "get_weather",
              "arguments": "{\"location\": \"New York\", \"unit\": \"celsius\"}"
            }
          }
        ]
      },
      "finish_reason": "tool_calls"
    }
  ],
  "usage": {
    "prompt_tokens": 10,
    "completion_tokens": 15,
    "total_tokens": 25
  }
}
```

---

## 3. ElevenLabs Custom LLM Configuration

### 3.1 Configuration Parameters

When setting up a custom LLM in the ElevenLabs agents dashboard:

1. **Server URL**: The base URL of your custom LLM endpoint
   - Example: `https://custom-llm-service.example.com`

2. **Model**: The model identifier to send in requests
   - Example: `gpt-4-custom` or `claude-3-sonnet`

3. **Secret/API Key**: Authentication credentials
   - Stored securely in ElevenLabs' secret storage
   - Can be referenced in custom headers

4. **Custom LLM Extra Body** (Optional): Boolean flag to enable passing additional data
   - When true: ElevenLabs can pass extra context in request body
   - When true: Required for receiving user identity information
   - Default: false

5. **Token Usage Limit** (Optional):
   - Example: 5000 tokens per conversation
   - Helps manage costs and prevent runaway responses

### 3.2 API Type Support

```json
{
  "api_type": "chat_completions"  // Required: "chat_completions" or "responses"
}
```

ElevenLabs supports:
- `chat_completions`: OpenAI-style chat completions API
- `responses`: Newer OpenAI Responses API (for compatible LLMs)

---

## 4. Authentication Mechanisms

### 4.1 Custom LLM Authentication

ElevenLabs does **not** use standard Authorization headers for your custom endpoint. Instead:

1. **Secret Storage Method** (Recommended):
   - Create a secret in the ElevenLabs agent dashboard
   - Reference the secret in custom headers
   - Format: `header_name: secret::secret_key_name`

2. **Custom Headers**:
   - Add custom authentication headers to your endpoint configuration
   - Example: `X-Custom-Auth: secret::my_api_key`

3. **Query Parameters** (Alternative):
   - Pass credentials as query parameters
   - Less secure, avoid for sensitive credentials

### 4.2 ElevenLabs Internal Authentication

For ElevenLabs' native API:
- Uses `xi-api-key` header (NOT Authorization)
- Format: `xi-api-key: your-elevenlabs-api-key`

---

## 5. ElevenLabs-Specific Parameters and Headers

### 5.1 Custom Request Headers

When ElevenLabs calls your custom LLM, you can configure custom headers:

```
X-Custom-Auth: {secret_value}
X-Agent-ID: {agent_id}
X-User-Context: {serialized_json}
X-Conversation-ID: {conversation_id}
Authorization: Bearer {token}  // Custom, not standard
```

### 5.2 Extra Body Parameters

When "Custom LLM Extra Body" is enabled, ElevenLabs may include:

```json
{
  "model": "your-model",
  "messages": [...],
  "extra": {
    "user_id": "user_123",
    "agent_id": "agent_456",
    "conversation_id": "conv_789",
    "metadata": {
      "phone_number": "+1234567890",
      "custom_field": "value"
    }
  }
}
```

### 5.3 Required vs Optional Fields

**Always included by ElevenLabs:**
- `model`
- `messages`

**Conditional (based on agent configuration):**
- `tools` (if agent has system tools configured)
- `temperature`, `max_tokens`, etc. (if explicitly set)
- `extra` (if Custom LLM Extra Body is enabled)

---

## 6. ElevenLabs TTS Integration and Streaming

### 6.1 Data Flow: LLM → TTS → Audio Streaming

```
User Input (Voice)
        ↓
    STT (Speech-to-Text)
        ↓
    Custom LLM (Streaming)
        ↓
    ElevenLabs TTS (Real-time)
        ↓
    Audio Stream (WebSocket)
        ↓
    Client Playback
```

### 6.2 Streaming Response Requirements

For optimal TTS performance:
- Support `stream: true` parameter
- Return tokens incrementally
- Maintain low latency (target: <100ms between chunks)
- Use proper SSE formatting
- Return content in natural chunks (complete sentences preferred)

### 6.3 Buffer Words Optimization

For slower custom LLMs, use buffer words to maintain conversational flow:

```python
# Example: Slow LLM processing
if processing:
    return_initial_response = "... "  # Ellipsis + space
    # This allows TTS to start generating while LLM continues
    # Must include the space to prevent concatenation issues
```

**Key Point:** The trailing space is crucial to prevent audio distortion from concatenation.

### 6.4 WebSocket TTS Streaming

ElevenLabs provides WebSocket API for real-time TTS:

```
wss://api.elevenlabs.io/v1/text-to-speech/{voice_id}/stream-input
```

**Features:**
- Multi-context support (5 concurrent contexts per connection)
- Real-time audio generation
- Voice customization (speed, stability)
- Word-level timestamps available
- Handles interruptions gracefully

---

## 7. Server-Sent Events (SSE) Support

### 7.1 SSE Format for Chat Completions

ElevenLabs fully supports SSE streaming:

**HTTP Response Headers:**
```
Content-Type: text/event-stream
Cache-Control: no-cache
Connection: keep-alive
Transfer-Encoding: chunked
```

**Message Format:**
```
event: message
data: {"id":"...", "object":"chat.completion.chunk", ...}

event: message
data: [DONE]
```

### 7.2 Implementation Example (Python/FastAPI)

```python
from fastapi import FastAPI
from fastapi.responses import StreamingResponse
import json

app = FastAPI()

@app.post("/v1/chat/completions")
async def chat_completions(request: dict):
    stream = request.get("stream", False)

    if stream:
        return StreamingResponse(
            generate_stream(request),
            media_type="text/event-stream"
        )
    else:
        return await generate_non_streaming(request)

async def generate_stream(request):
    model = request.get("model")
    messages = request.get("messages")

    # Generate response incrementally
    response_text = await llm_generate(messages)

    for i, token in enumerate(response_text.split()):
        chunk = {
            "id": "chatcmpl-123",
            "object": "chat.completion.chunk",
            "created": 1677649420,
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {
                    "role": "assistant" if i == 0 else None,
                    "content": token + " "
                },
                "finish_reason": None
            }]
        }
        yield f"data: {json.dumps(chunk)}\n\n"

    # Final chunk
    final_chunk = {
        "id": "chatcmpl-123",
        "object": "chat.completion.chunk",
        "created": 1677649420,
        "model": model,
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": "stop"
        }]
    }
    yield f"data: {json.dumps(final_chunk)}\n\n"
    yield "data: [DONE]\n\n"
```

### 7.3 Alternative Streaming Mechanisms

While SSE is preferred, other options include:
- **HTTP Long-Polling**: Less efficient but works everywhere
- **WebSockets**: For bidirectional communication
- **gRPC Streaming**: For high-performance scenarios

---

## 8. OpenAI /v1/models Endpoint (Optional but Recommended)

Some ElevenLabs integrations may query available models:

```
GET {BASE_URL}/v1/models
```

**Response Format:**

```json
{
  "object": "list",
  "data": [
    {
      "id": "gpt-4-custom",
      "object": "model",
      "created": 1677649420,
      "owned_by": "your-organization",
      "permission": [{
        "id": "modelperm-123",
        "object": "model_permission",
        "created": 1677649420,
        "allow_create_engine": false,
        "allow_sampling": true,
        "allow_logprobs": true,
        "allow_search_indices": false,
        "allow_view": true,
        "allow_fine_tuning": false,
        "organization": "*",
        "group_id": null,
        "is_blocking": false
      }],
      "root": "gpt-4-custom",
      "parent": null
    }
  ]
}
```

---

## 9. Implementation Example: FastAPI Custom LLM Server

### 9.1 Complete Server Implementation

```python
from fastapi import FastAPI, HTTPException
from fastapi.responses import StreamingResponse
from pydantic import BaseModel
from typing import Optional, List, Dict, Any
import json
import asyncio
import uuid
from datetime import datetime

app = FastAPI()

# Request models
class Message(BaseModel):
    role: str  # "system", "user", "assistant"
    content: str

class Tool(BaseModel):
    type: str = "function"
    function: Dict[str, Any]

class ChatCompletionRequest(BaseModel):
    model: str
    messages: List[Message]
    temperature: Optional[float] = 0.7
    max_tokens: Optional[int] = 2000
    stream: Optional[bool] = False
    tools: Optional[List[Tool]] = None
    tool_choice: Optional[str] = None
    extra: Optional[Dict[str, Any]] = None  # ElevenLabs extra body

@app.post("/v1/chat/completions")
async def chat_completions(request: ChatCompletionRequest):
    """Main chat completion endpoint"""

    # Validate model
    if request.model not in ["gpt-4-custom", "claude-3-sonnet"]:
        raise HTTPException(status_code=400, detail="Invalid model")

    # Process extra body data (user context, agent ID, etc.)
    user_context = request.extra or {}

    if request.stream:
        return StreamingResponse(
            generate_stream(request, user_context),
            media_type="text/event-stream"
        )
    else:
        return await generate_non_streaming(request, user_context)

async def generate_non_streaming(request: ChatCompletionRequest, context: Dict):
    """Non-streaming response"""

    completion_id = f"chatcmpl-{uuid.uuid4().hex[:12]}"

    try:
        # Call your LLM
        response_text = await call_your_llm(
            messages=request.messages,
            model=request.model,
            temperature=request.temperature,
            max_tokens=request.max_tokens,
            context=context
        )

        response = {
            "id": completion_id,
            "object": "chat.completion",
            "created": int(datetime.now().timestamp()),
            "model": request.model,
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": response_text
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": count_tokens(request.messages),
                "completion_tokens": count_tokens(response_text),
                "total_tokens": count_tokens(request.messages) + count_tokens(response_text)
            }
        }

        return response

    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

async def generate_stream(request: ChatCompletionRequest, context: Dict):
    """Streaming response generator"""

    completion_id = f"chatcmpl-{uuid.uuid4().hex[:12]}"
    created = int(datetime.now().timestamp())

    try:
        # Call streaming LLM
        async for token in stream_your_llm(
            messages=request.messages,
            model=request.model,
            temperature=request.temperature,
            max_tokens=request.max_tokens,
            context=context
        ):
            chunk = {
                "id": completion_id,
                "object": "chat.completion.chunk",
                "created": created,
                "model": request.model,
                "choices": [{
                    "index": 0,
                    "delta": {"content": token},
                    "finish_reason": None
                }]
            }
            yield f"data: {json.dumps(chunk)}\n\n"
            await asyncio.sleep(0)  # Yield control

        # Final chunk
        final_chunk = {
            "id": completion_id,
            "object": "chat.completion.chunk",
            "created": created,
            "model": request.model,
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "stop"
            }]
        }
        yield f"data: {json.dumps(final_chunk)}\n\n"
        yield "data: [DONE]\n\n"

    except Exception as e:
        error_chunk = {
            "id": completion_id,
            "object": "chat.completion.chunk",
            "created": created,
            "model": request.model,
            "error": {"message": str(e)}
        }
        yield f"data: {json.dumps(error_chunk)}\n\n"

@app.get("/v1/models")
async def list_models():
    """List available models (optional but recommended)"""

    return {
        "object": "list",
        "data": [
            {
                "id": "gpt-4-custom",
                "object": "model",
                "created": int(datetime.now().timestamp()),
                "owned_by": "your-organization"
            },
            {
                "id": "claude-3-sonnet",
                "object": "model",
                "created": int(datetime.now().timestamp()),
                "owned_by": "your-organization"
            }
        ]
    }

async def call_your_llm(messages, model, temperature, max_tokens, context):
    """Call your actual LLM implementation"""
    # Implement your LLM logic here
    # This could call OpenAI, Anthropic, local model, etc.
    return "Sample response from LLM"

async def stream_your_llm(messages, model, temperature, max_tokens, context):
    """Stream from your actual LLM implementation"""
    # Implement streaming logic
    response = "Sample streaming response"
    for word in response.split():
        yield word + " "
        await asyncio.sleep(0.1)

def count_tokens(text_or_messages):
    """Simple token counting (use tokenizer for accuracy)"""
    if isinstance(text_or_messages, list):
        total = sum(len(str(m.get("content", "")).split()) for m in text_or_messages)
    else:
        total = len(str(text_or_messages).split())
    return total

if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="0.0.0.0", port=8000)
```

### 9.2 Deployment with ngrok

```bash
# Install ngrok and FastAPI
pip install fastapi uvicorn ngrok

# Run FastAPI server
uvicorn main:app --host 0.0.0.0 --port 8000

# In another terminal, expose with ngrok
ngrok http 8000
# Output: https://your-app.ngrok.io

# Use this URL in ElevenLabs agent configuration
```

---

## 10. Best Practices and Performance Optimization

### 10.1 Response Time Targets

- **Initial response**: <100ms (time to first token)
- **Token generation**: <50ms per token
- **Complete response**: <2 seconds for typical conversation

### 10.2 Streaming Best Practices

1. **Send tokens incrementally** - Don't buffer entire response
2. **Include role in first chunk only** - Set "role" only in first delta
3. **Use proper SSE formatting** - Include newlines and proper encoding
4. **Handle backpressure** - Respect client flow control
5. **Implement timeouts** - Prevent hanging connections

### 10.3 Error Handling

```json
{
  "error": {
    "message": "Invalid request format",
    "type": "invalid_request_error",
    "param": "messages",
    "code": "invalid_value"
  }
}
```

### 10.4 Latency Optimization with Buffer Words

For slow LLMs:
```python
# Return immediately with buffer text
initial_response = "... "  # Note the space
# Then continue generating
```

### 10.5 Token Usage Limits

ElevenLabs allows setting limits per conversation:
- Default: No limit
- Recommended for testing: 5000 tokens
- Can prevent runaway costs with slow LLMs

---

## 11. Common Integration Patterns

### 11.1 Using OpenAI as Custom LLM

```python
from openai import OpenAI
import os

class OpenAILLM:
    def __init__(self):
        self.client = OpenAI(api_key=os.getenv("OPENAI_API_KEY"))

    async def generate(self, messages, model, **kwargs):
        response = self.client.chat.completions.create(
            model=model,
            messages=messages,
            **kwargs
        )
        return response.choices[0].message.content

    async def stream(self, messages, model, **kwargs):
        kwargs['stream'] = True
        response = self.client.chat.completions.create(
            model=model,
            messages=messages,
            **kwargs
        )
        for chunk in response:
            if chunk.choices[0].delta.content:
                yield chunk.choices[0].delta.content
```

### 11.2 Using Anthropic Claude

```python
from anthropic import Anthropic

class ClaudeLLM:
    def __init__(self):
        self.client = Anthropic(api_key=os.getenv("ANTHROPIC_API_KEY"))

    async def generate(self, messages, model="claude-3-sonnet-20240229", **kwargs):
        response = self.client.messages.create(
            model=model,
            messages=messages,
            max_tokens=kwargs.get("max_tokens", 1024)
        )
        return response.content[0].text

    async def stream(self, messages, model="claude-3-sonnet-20240229", **kwargs):
        with self.client.messages.stream(
            model=model,
            messages=messages,
            max_tokens=kwargs.get("max_tokens", 1024)
        ) as stream:
            for text in stream.text_stream:
                yield text
```

### 11.3 Using LiteLLM Proxy (Multi-Provider)

```bash
# Install litellm
pip install litellm

# Create config.yaml
model_list:
  - model_name: gpt-4-custom
    litellm_params:
      model: gpt-4
      api_key: ${OPENAI_API_KEY}
  - model_name: claude-custom
    litellm_params:
      model: claude-3-sonnet-20240229
      api_key: ${ANTHROPIC_API_KEY}

# Start LiteLLM proxy
litellm --config config.yaml --port 8000

# Use in ElevenLabs: https://your-app.ngrok.io
```

---

## 12. Security Considerations

### 12.1 API Key Management

- Store secrets in ElevenLabs secret manager
- Use environment variables in deployment
- Rotate keys regularly
- Never commit credentials to version control

### 12.2 Input Validation

- Validate message format
- Check model names against whitelist
- Limit max_tokens to prevent abuse
- Rate limit requests per agent/user

### 12.3 Authentication Headers

```python
from fastapi import Header, HTTPException

@app.post("/v1/chat/completions")
async def chat_completions(
    request: ChatCompletionRequest,
    x_custom_auth: str = Header(None)
):
    if not x_custom_auth:
        raise HTTPException(status_code=401, detail="Missing authentication")

    if x_custom_auth != os.getenv("EXPECTED_SECRET"):
        raise HTTPException(status_code=403, detail="Invalid authentication")

    # Process request
```

---

## 13. ElevenLabs Agent Customization

### 13.1 Tools and System Tools

ElevenLabs automatically includes configured tools in the `tools` parameter:

```json
{
  "model": "your-model",
  "messages": [...],
  "tools": [
    {
      "type": "function",
      "function": {
        "name": "get_weather",
        "description": "Get weather for a location",
        "parameters": {
          "type": "object",
          "properties": {
            "location": {"type": "string"},
            "unit": {"type": "string", "enum": ["celsius", "fahrenheit"]}
          },
          "required": ["location"]
        }
      }
    }
  ]
}
```

### 13.2 System Prompts and Context

- Maximum system prompt size: 2MB
- Includes agent instructions and knowledge base
- Can use dynamic variables for personalization
- Use overrides for per-conversation customization

### 13.3 Conversation Overrides

Pass custom values per conversation:

```python
overrides = {
    "system_prompt": "Custom system prompt for this user",
    "first_message": "Hello, this is a custom greeting",
    "llm": "gpt-4-custom",
    "tts_voice_id": "custom_voice_id"
}
```

### 13.4 Dynamic Variables

Inject runtime values:

```json
{
  "user_id": "{{user_id}}",
  "account_type": "{{account_type}}",
  "agent_id": "{{agent_id}}"
}
```

---

## 14. Troubleshooting Common Issues

### 14.1 Connection Issues

**Problem:** ElevenLabs cannot reach custom LLM endpoint

**Solutions:**
- Verify endpoint is publicly accessible
- Check ngrok tunnel is active
- Ensure CORS headers if needed
- Check firewall/network restrictions

### 14.2 Streaming Issues

**Problem:** Streaming responses not working

**Solutions:**
- Verify `Content-Type: text/event-stream`
- Ensure proper SSE format with `data:` prefix
- Include trailing newlines: `\n\n`
- Don't buffer entire response

### 14.3 Token Usage

**Problem:** High token usage or cutoffs

**Solutions:**
- Check `max_tokens` parameter
- Implement buffer words for slow responses
- Optimize system prompt length
- Use appropriate `temperature` values

### 14.4 Tool Calling

**Problem:** Model not calling tools

**Solutions:**
- Verify tools format matches specification
- Ensure tool_choice parameter is set
- Check tool descriptions are clear
- Validate function schema syntax

---

## 15. References and Resources

### Official Documentation
- [ElevenLabs Agents Platform](https://elevenlabs.io/docs/agents-platform/overview)
- [ElevenLabs Custom LLM Guide](https://elevenlabs.io/docs/agents-platform/customization/llm/custom-llm)
- [ElevenLabs API Reference](https://elevenlabs.io/docs/api-reference/introduction)
- [OpenAI API Documentation](https://platform.openai.com/docs/api-reference/chat)
- [OpenAI Streaming Guide](https://platform.openai.com/docs/guides/streaming-responses)

### Third-Party Tools
- [LiteLLM](https://docs.litellm.ai/docs/providers/litellm_proxy) - Multi-provider LLM proxy
- [vLLM](https://docs.vllm.ai/en/stable/serving/openai_compatible_server/) - OpenAI-compatible server
- [Pipecat](https://docs.pipecat.ai/server/services/tts/elevenlabs) - Real-time AI framework

### Example Implementations
- [ElevenLabs GitHub Docs](https://github.com/elevenlabs/elevenlabs-docs)
- [elevenlabs-zep-example](https://github.com/elevenlabs/elevenlabs-zep-example)
- OpenAI Cookbook examples

---

## 16. Quick Reference Checklist

When implementing a custom LLM for ElevenLabs:

- [ ] Endpoint is publicly accessible at `BASE_URL/v1/chat/completions`
- [ ] Supports POST requests with JSON body
- [ ] Implements OpenAI-compatible request/response format
- [ ] Supports both streaming (`stream: true`) and non-streaming responses
- [ ] Returns proper SSE format for streaming (text/event-stream content-type)
- [ ] Includes `id`, `object`, `created`, `model`, `choices`, `usage` in responses
- [ ] Implements `/v1/models` endpoint for model listing
- [ ] Handles authentication via custom headers or query params
- [ ] Extracts and uses `extra` body data when available
- [ ] Implements tool calling support if tools are configured
- [ ] Handles `max_tokens` and `temperature` parameters
- [ ] Implements proper error handling
- [ ] Response time < 100ms for first token
- [ ] Supports buffer words optimization ("... ") for slow LLMs
- [ ] Deployed and accessible (ngrok or public URL)
- [ ] Configured in ElevenLabs agent dashboard with URL and model name

---

**Document Generated:** 2026-02-04
**Research Scope:** ElevenLabs Custom LLM API Specification, OpenAI Compatibility, TTS Integration
**Status:** Comprehensive Research Complete
