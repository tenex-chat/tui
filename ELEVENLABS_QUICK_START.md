# ElevenLabs Custom LLM - Quick Start Guide

A fast-track reference for implementing a custom LLM endpoint for ElevenLabs Agents Platform.

---

## 1-Minute Overview

ElevenLabs Agents Platform requires custom LLM servers to implement **OpenAI-compatible** endpoints:

```
POST {YOUR_URL}/v1/chat/completions
```

The endpoint must:
- Accept OpenAI-style JSON requests
- Return OpenAI-compatible responses
- Support streaming (Server-Sent Events)
- Return proper HTTP headers

---

## 5-Minute Setup

### Step 1: Create FastAPI Server

```python
# server.py
from fastapi import FastAPI
from fastapi.responses import StreamingResponse
from pydantic import BaseModel
from typing import List, Optional
import json
from openai import AsyncOpenAI

app = FastAPI()
client = AsyncOpenAI()

class Message(BaseModel):
    role: str
    content: str

class ChatRequest(BaseModel):
    model: str
    messages: List[Message]
    stream: Optional[bool] = False

@app.post("/v1/chat/completions")
async def chat(req: ChatRequest):
    if req.stream:
        return StreamingResponse(stream_chat(req), media_type="text/event-stream")

    resp = await client.chat.completions.create(
        model=req.model,
        messages=[{"role": m.role, "content": m.content} for m in req.messages]
    )

    return {
        "id": "chatcmpl-123",
        "object": "chat.completion",
        "created": 1234567890,
        "model": req.model,
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": resp.choices[0].message.content},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30}
    }

async def stream_chat(req):
    stream = await client.chat.completions.create(
        model=req.model,
        messages=[{"role": m.role, "content": m.content} for m in req.messages],
        stream=True
    )

    first = True
    async for chunk in stream:
        delta = {}
        if first:
            delta["role"] = "assistant"
            first = False
        if chunk.choices[0].delta.content:
            delta["content"] = chunk.choices[0].delta.content

        data = {
            "id": "chatcmpl-123",
            "object": "chat.completion.chunk",
            "choices": [{"index": 0, "delta": delta, "finish_reason": None}]
        }
        yield f"data: {json.dumps(data)}\n\n"

    yield f"data: {json.dumps({'choices': [{'delta': {}, 'finish_reason': 'stop'}]})}\n\n"
    yield "data: [DONE]\n\n"

@app.get("/v1/models")
async def models():
    return {
        "object": "list",
        "data": [{"id": "gpt-4", "object": "model", "owned_by": "openai"}]
    }
```

### Step 2: Run Locally

```bash
pip install fastapi uvicorn openai
export OPENAI_API_KEY="sk-..."
python -m uvicorn server:app --reload
# Running on http://localhost:8000
```

### Step 3: Expose with ngrok

```bash
pip install ngrok
ngrok http 8000
# Copy the https://xxx.ngrok.io URL
```

### Step 4: Configure in ElevenLabs

1. Open your AI Agent in ElevenLabs dashboard
2. Go to **Customization** → **Model (LLM)** → **Change Model**
3. Select **Custom LLM**
4. Paste the ngrok URL: `https://xxx.ngrok.io`
5. Set Model: `gpt-4`
6. Click **Save**
7. Test with a conversation

---

## Request/Response Format Cheat Sheet

### Non-Streaming Request

```json
{
  "model": "gpt-4",
  "messages": [
    {"role": "system", "content": "You are helpful"},
    {"role": "user", "content": "Hello"}
  ],
  "temperature": 0.7,
  "max_tokens": 2000,
  "stream": false
}
```

### Non-Streaming Response

```json
{
  "id": "chatcmpl-123",
  "object": "chat.completion",
  "created": 1234567890,
  "model": "gpt-4",
  "choices": [{
    "index": 0,
    "message": {
      "role": "assistant",
      "content": "Hello! How can I help?"
    },
    "finish_reason": "stop"
  }],
  "usage": {
    "prompt_tokens": 10,
    "completion_tokens": 15,
    "total_tokens": 25
  }
}
```

### Streaming Response (SSE)

```
data: {"id":"chatcmpl-123","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":"Hello"},"finish_reason":null}]}

data: {"id":"chatcmpl-123","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":" there"},"finish_reason":null}]}

data: {"id":"chatcmpl-123","object":"chat.completion.chunk","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}

data: [DONE]
```

---

## Testing

### With cURL

```bash
curl -X POST "http://localhost:8000/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4",
    "messages": [{"role": "user", "content": "Hi"}],
    "stream": false
  }'
```

### With Python OpenAI Client

```python
from openai import OpenAI

client = OpenAI(api_key="unused", base_url="http://localhost:8000")

response = client.chat.completions.create(
    model="gpt-4",
    messages=[{"role": "user", "content": "Hi"}]
)
print(response.choices[0].message.content)

# Streaming
stream = client.chat.completions.create(
    model="gpt-4",
    messages=[{"role": "user", "content": "Hi"}],
    stream=True
)
for chunk in stream:
    print(chunk.choices[0].delta.content, end="")
```

---

## Common Gotchas

| Issue | Solution |
|-------|----------|
| Streaming not working | Use `media_type="text/event-stream"`, include `data:` prefix, end with `\n\n` |
| Connection timeout | Send first chunk within 5 seconds, use `await asyncio.sleep(0)` to yield control |
| Empty delta in stream | Only include `role` in first chunk, only include `content` if not empty |
| ngrok URL expired | Keep ngrok terminal open, use paid ngrok account for persistence |
| ElevenLabs can't reach URL | Ensure ngrok is running, check firewall, verify URL is accessible |
| High token costs | Limit `max_tokens`, optimize system prompt, use buffer words for slow LLMs |

---

## Buffer Words Optimization

For slow custom LLMs, return "... " immediately to keep conversation flowing:

```python
async def stream_with_buffer():
    # Return buffer immediately
    initial = {
        "choices": [{"index": 0, "delta": {"role": "assistant", "content": "... "}}]
    }
    yield f"data: {json.dumps(initial)}\n\n"

    # Then continue generating slower response
    full_response = await slow_llm_call()
    for word in full_response.split():
        chunk = {
            "choices": [{"index": 0, "delta": {"content": word + " "}}]
        }
        yield f"data: {json.dumps(chunk)}\n\n"

    yield f"data: {json.dumps({'choices': [{'delta': {}, 'finish_reason': 'stop'}]})}\n\n"
    yield "data: [DONE]\n\n"
```

**Important:** Always include the trailing space after "..."

---

## With Tools/Functions

ElevenLabs automatically includes configured tools:

```json
{
  "model": "gpt-4",
  "messages": [...],
  "tools": [{
    "type": "function",
    "function": {
      "name": "get_weather",
      "description": "Get weather for a location",
      "parameters": {
        "type": "object",
        "properties": {
          "location": {"type": "string"}
        },
        "required": ["location"]
      }
    }
  }]
}
```

Return tool calls:

```json
{
  "choices": [{
    "message": {
      "role": "assistant",
      "tool_calls": [{
        "id": "call_123",
        "type": "function",
        "function": {
          "name": "get_weather",
          "arguments": "{\"location\": \"NYC\"}"
        }
      }]
    },
    "finish_reason": "tool_calls"
  }]
}
```

---

## Authentication

ElevenLabs supports custom headers:

1. Create a secret in ElevenLabs dashboard (Agent → Secrets → Add Secret)
2. Reference in custom header configuration
3. Your server receives:

```python
@app.post("/v1/chat/completions")
async def chat(request: ChatCompletionRequest, x_custom_auth: str = Header(None)):
    if x_custom_auth != expected_token:
        raise HTTPException(status_code=401)
    # Process request
```

---

## Deployment Options

### ngrok (Development)

```bash
ngrok http 8000
# Use https://xxx.ngrok.io
```

### Docker (Any Cloud)

```dockerfile
FROM python:3.11-slim
WORKDIR /app
COPY . .
RUN pip install -r requirements.txt
CMD ["uvicorn", "server:app", "--host", "0.0.0.0"]
```

```bash
docker build -t custom-llm .
docker run -p 8000:8000 -e OPENAI_API_KEY=$OPENAI_API_KEY custom-llm
```

### Railway.app (Easiest)

1. Push to GitHub
2. Connect Railway to repo
3. Add env variables
4. Deploy (automatic)

### AWS Lambda + API Gateway

```python
from mangum import Mangum
from server import app

handler = Mangum(app)
```

---

## Monitoring

### Health Check Endpoint

```python
@app.get("/health")
async def health():
    return {"status": "ok"}
```

Test:
```bash
curl http://localhost:8000/health
```

### Response Time Tracking

```python
import time

@app.post("/v1/chat/completions")
async def chat(request: ChatCompletionRequest):
    start = time.time()
    # ... process ...
    duration = time.time() - start
    print(f"Response time: {duration:.2f}s")
```

---

## Performance Targets

- **First token latency**: < 100ms
- **Per-token latency**: < 50ms
- **Total response**: < 2 seconds for typical conversation
- **Streaming throughput**: > 50 tokens/second

---

## Troubleshooting Checklist

- [ ] FastAPI server running: `http://localhost:8000/health` returns 200
- [ ] ngrok tunnel active: Check ngrok terminal
- [ ] Endpoint accessible: `curl https://xxx.ngrok.io/health`
- [ ] OpenAI API key set: `echo $OPENAI_API_KEY`
- [ ] Request format correct: Check against examples above
- [ ] Streaming headers set: `Content-Type: text/event-stream`
- [ ] SSE format correct: Each line starts with `data:`, ends with `\n\n`
- [ ] Response includes required fields: `id`, `object`, `created`, `model`, `choices`, `usage`
- [ ] ElevenLabs URL saved: Agent → Customization → Model → URL field
- [ ] Agent tested: Send test message in ElevenLabs UI

---

## Next Steps

1. **Get this working locally** with the 5-minute setup
2. **Test with OpenAI API** to ensure format is correct
3. **Deploy to production** (ngrok for testing, proper host for production)
4. **Add authentication** if handling sensitive data
5. **Monitor performance** and adjust buffer words if needed
6. **Scale as needed** using appropriate hosting solution

---

## Complete Working Example

Save as `app.py`:

```python
from fastapi import FastAPI
from fastapi.responses import StreamingResponse
from pydantic import BaseModel
from typing import List, Optional
import json
from openai import AsyncOpenAI
import uvicorn

app = FastAPI()
client = AsyncOpenAI()

class Message(BaseModel):
    role: str
    content: str

class ChatRequest(BaseModel):
    model: str
    messages: List[Message]
    stream: Optional[bool] = False

@app.post("/v1/chat/completions")
async def chat(req: ChatRequest):
    if req.stream:
        return StreamingResponse(stream_chat(req), media_type="text/event-stream")

    msgs = [{"role": m.role, "content": m.content} for m in req.messages]
    resp = await client.chat.completions.create(model=req.model, messages=msgs)

    return {
        "id": "chatcmpl-123", "object": "chat.completion", "created": 1234567890,
        "model": req.model,
        "choices": [{"index": 0, "message": {"role": "assistant", "content": resp.choices[0].message.content}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30}
    }

async def stream_chat(req):
    msgs = [{"role": m.role, "content": m.content} for m in req.messages]
    stream = await client.chat.completions.create(model=req.model, messages=msgs, stream=True)

    first = True
    async for chunk in stream:
        delta = {"role": "assistant"} if first else {}
        if chunk.choices[0].delta.content:
            delta["content"] = chunk.choices[0].delta.content
        first = False

        yield f"data: {json.dumps({'id': 'chatcmpl-123', 'object': 'chat.completion.chunk', 'choices': [{'index': 0, 'delta': delta, 'finish_reason': None}]})}\n\n"

    yield f"data: {json.dumps({'choices': [{'delta': {}, 'finish_reason': 'stop'}]})}\n\n"
    yield "data: [DONE]\n\n"

@app.get("/v1/models")
async def models():
    return {"object": "list", "data": [{"id": "gpt-4", "object": "model", "owned_by": "openai"}]}

@app.get("/health")
async def health():
    return {"status": "ok"}

if __name__ == "__main__":
    uvicorn.run(app, host="0.0.0.0", port=8000)
```

Run:
```bash
pip install fastapi uvicorn openai
export OPENAI_API_KEY="sk-..."
python app.py
# Open new terminal
ngrok http 8000
# Copy https://xxx.ngrok.io to ElevenLabs agent
```

---

## Quick Reference URLs

- **ElevenLabs Custom LLM Docs:** https://elevenlabs.io/docs/agents-platform/customization/llm/custom-llm
- **OpenAI Chat Completions API:** https://platform.openai.com/docs/api-reference/chat
- **OpenAI Streaming Guide:** https://platform.openai.com/docs/guides/streaming-responses
- **FastAPI Docs:** https://fastapi.tiangolo.com/
- **ngrok:** https://ngrok.com/

---

**Last Updated:** 2026-02-04
**Quick Reference Version:** v1.0
