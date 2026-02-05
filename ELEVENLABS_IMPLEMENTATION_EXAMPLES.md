# ElevenLabs Custom LLM - Implementation Examples and Code Snippets

This document provides practical code examples for implementing custom LLM endpoints compatible with ElevenLabs Agents Platform.

---

## Part 1: Basic FastAPI Implementation

### 1.1 Minimal Working Example

```python
# main.py
from fastapi import FastAPI
from fastapi.responses import StreamingResponse
from pydantic import BaseModel
from typing import Optional, List, Any, Dict
import json
import uuid
from datetime import datetime

app = FastAPI()

class Message(BaseModel):
    role: str
    content: str

class ChatRequest(BaseModel):
    model: str
    messages: List[Message]
    temperature: Optional[float] = 0.7
    stream: Optional[bool] = False
    max_tokens: Optional[int] = 2000

@app.post("/v1/chat/completions")
async def chat(request: ChatRequest):
    """Minimal chat completion endpoint"""

    # Your LLM call here (using OpenAI as example)
    from openai import OpenAI

    client = OpenAI()

    if request.stream:
        return StreamingResponse(
            stream_response(request, client),
            media_type="text/event-stream"
        )

    response = client.chat.completions.create(
        model=request.model,
        messages=[{"role": m.role, "content": m.content} for m in request.messages],
        temperature=request.temperature,
        max_tokens=request.max_tokens
    )

    return {
        "id": f"chatcmpl-{uuid.uuid4().hex[:12]}",
        "object": "chat.completion",
        "created": int(datetime.now().timestamp()),
        "model": request.model,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": response.choices[0].message.content
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": response.usage.prompt_tokens,
            "completion_tokens": response.usage.completion_tokens,
            "total_tokens": response.usage.total_tokens
        }
    }

async def stream_response(request: ChatRequest, client):
    """Stream response with proper SSE format"""

    stream = client.chat.completions.create(
        model=request.model,
        messages=[{"role": m.role, "content": m.content} for m in request.messages],
        temperature=request.temperature,
        max_tokens=request.max_tokens,
        stream=True
    )

    first_chunk = True
    for chunk in stream:
        delta_content = ""
        if chunk.choices[0].delta.content:
            delta_content = chunk.choices[0].delta.content

        data = {
            "id": f"chatcmpl-{uuid.uuid4().hex[:12]}",
            "object": "chat.completion.chunk",
            "created": int(datetime.now().timestamp()),
            "model": request.model,
            "choices": [{
                "index": 0,
                "delta": {
                    "role": "assistant" if first_chunk else None,
                    "content": delta_content if delta_content else None
                },
                "finish_reason": None
            }]
        }

        # Clean up None values
        if data["choices"][0]["delta"]["role"] is None:
            del data["choices"][0]["delta"]["role"]
        if data["choices"][0]["delta"]["content"] is None:
            del data["choices"][0]["delta"]["content"]

        yield f"data: {json.dumps(data)}\n\n"
        first_chunk = False

    # Send final chunk
    yield f"data: {json.dumps({'id': f'chatcmpl-{uuid.uuid4().hex[:12]}', 'object': 'chat.completion.chunk', 'choices': [{'index': 0, 'delta': {}, 'finish_reason': 'stop'}]})}\n\n"
    yield "data: [DONE]\n\n"

@app.get("/v1/models")
async def list_models():
    """List available models"""
    return {
        "object": "list",
        "data": [
            {"id": "gpt-4", "object": "model", "created": int(datetime.now().timestamp()), "owned_by": "openai"},
            {"id": "gpt-4-turbo", "object": "model", "created": int(datetime.now().timestamp()), "owned_by": "openai"}
        ]
    }

if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="0.0.0.0", port=8000)
```

### 1.2 Running the Server

```bash
# Install dependencies
pip install fastapi uvicorn openai

# Set your OpenAI API key
export OPENAI_API_KEY="sk-..."

# Run the server
python main.py
# Server running on http://localhost:8000

# In another terminal, expose with ngrok
ngrok http 8000
# Use the https://xxx.ngrok.io URL in ElevenLabs
```

---

## Part 2: Production-Ready Implementation

### 2.1 Comprehensive LLM Wrapper with Error Handling

```python
# llm_service.py
import os
import json
import asyncio
from typing import Optional, List, Dict, Any, AsyncIterator
from openai import OpenAI, AsyncOpenAI
from anthropic import Anthropic, AsyncAnthropic
import logging

logger = logging.getLogger(__name__)

class LLMProvider:
    """Abstract base class for LLM providers"""

    async def generate(
        self,
        messages: List[Dict[str, str]],
        model: str,
        temperature: float = 0.7,
        max_tokens: int = 2000,
        tools: Optional[List[Dict]] = None,
        **kwargs
    ) -> str:
        raise NotImplementedError

    async def stream(
        self,
        messages: List[Dict[str, str]],
        model: str,
        temperature: float = 0.7,
        max_tokens: int = 2000,
        tools: Optional[List[Dict]] = None,
        **kwargs
    ) -> AsyncIterator[str]:
        raise NotImplementedError

class OpenAIProvider(LLMProvider):
    """OpenAI provider"""

    def __init__(self, api_key: Optional[str] = None):
        self.client = AsyncOpenAI(api_key=api_key or os.getenv("OPENAI_API_KEY"))

    async def generate(
        self,
        messages: List[Dict[str, str]],
        model: str,
        temperature: float = 0.7,
        max_tokens: int = 2000,
        tools: Optional[List[Dict]] = None,
        **kwargs
    ) -> str:
        try:
            response = await self.client.chat.completions.create(
                model=model,
                messages=messages,
                temperature=temperature,
                max_tokens=max_tokens,
                tools=tools,
                timeout=30
            )
            return response.choices[0].message.content or ""
        except Exception as e:
            logger.error(f"OpenAI API error: {e}")
            raise

    async def stream(
        self,
        messages: List[Dict[str, str]],
        model: str,
        temperature: float = 0.7,
        max_tokens: int = 2000,
        tools: Optional[List[Dict]] = None,
        **kwargs
    ) -> AsyncIterator[str]:
        try:
            stream = await self.client.chat.completions.create(
                model=model,
                messages=messages,
                temperature=temperature,
                max_tokens=max_tokens,
                tools=tools,
                stream=True,
                timeout=30
            )

            async for chunk in stream:
                if chunk.choices[0].delta.content:
                    yield chunk.choices[0].delta.content
        except Exception as e:
            logger.error(f"OpenAI streaming error: {e}")
            raise

class AnthropicProvider(LLMProvider):
    """Anthropic Claude provider"""

    def __init__(self, api_key: Optional[str] = None):
        self.client = AsyncAnthropic(api_key=api_key or os.getenv("ANTHROPIC_API_KEY"))

    async def generate(
        self,
        messages: List[Dict[str, str]],
        model: str,
        temperature: float = 0.7,
        max_tokens: int = 2000,
        tools: Optional[List[Dict]] = None,
        **kwargs
    ) -> str:
        try:
            # Convert tools to Anthropic format if needed
            tool_list = None
            if tools:
                tool_list = [{"name": t["function"]["name"], "description": t["function"]["description"], "input_schema": t["function"]["parameters"]} for t in tools]

            response = await self.client.messages.create(
                model=model,
                messages=messages,
                temperature=temperature,
                max_tokens=max_tokens,
                tools=tool_list
            )
            return response.content[0].text
        except Exception as e:
            logger.error(f"Anthropic API error: {e}")
            raise

    async def stream(
        self,
        messages: List[Dict[str, str]],
        model: str,
        temperature: float = 0.7,
        max_tokens: int = 2000,
        tools: Optional[List[Dict]] = None,
        **kwargs
    ) -> AsyncIterator[str]:
        try:
            tool_list = None
            if tools:
                tool_list = [{"name": t["function"]["name"], "description": t["function"]["description"], "input_schema": t["function"]["parameters"]} for t in tools]

            async with self.client.messages.stream(
                model=model,
                messages=messages,
                temperature=temperature,
                max_tokens=max_tokens,
                tools=tool_list
            ) as stream:
                async for text in stream.text_stream:
                    yield text
        except Exception as e:
            logger.error(f"Anthropic streaming error: {e}")
            raise

class LLMRegistry:
    """Registry for available LLM providers"""

    def __init__(self):
        self.providers: Dict[str, LLMProvider] = {
            "openai": OpenAIProvider(),
            "anthropic": AnthropicProvider(),
        }

    def get_provider(self, model: str) -> LLMProvider:
        """Get provider for model"""
        if "gpt" in model.lower():
            return self.providers["openai"]
        elif "claude" in model.lower():
            return self.providers["anthropic"]
        else:
            # Default to OpenAI
            return self.providers["openai"]

    def register(self, name: str, provider: LLMProvider):
        """Register a new provider"""
        self.providers[name] = provider
```

### 2.2 Main FastAPI Server with Error Handling

```python
# app.py
from fastapi import FastAPI, HTTPException, Header
from fastapi.responses import StreamingResponse
from pydantic import BaseModel, Field
from typing import Optional, List, Dict, Any
import json
import uuid
from datetime import datetime
import logging
from llm_service import LLMRegistry

logger = logging.getLogger(__name__)
logging.basicConfig(level=logging.INFO)

app = FastAPI(title="Custom LLM API", version="1.0.0")
llm_registry = LLMRegistry()

# Request models
class Message(BaseModel):
    role: str
    content: str

class ToolFunction(BaseModel):
    name: str
    description: str
    parameters: Dict[str, Any]

class Tool(BaseModel):
    type: str = "function"
    function: ToolFunction

class ChatCompletionRequest(BaseModel):
    model: str
    messages: List[Message]
    temperature: Optional[float] = Field(default=0.7, ge=0, le=2)
    top_p: Optional[float] = Field(default=1.0, ge=0, le=1)
    max_tokens: Optional[int] = Field(default=2000, ge=1, le=4000)
    stream: Optional[bool] = False
    tools: Optional[List[Tool]] = None
    tool_choice: Optional[str] = None
    presence_penalty: Optional[float] = Field(default=0, ge=-2, le=2)
    frequency_penalty: Optional[float] = Field(default=0, ge=-2, le=2)
    extra: Optional[Dict[str, Any]] = None

@app.post("/v1/chat/completions")
async def chat_completions(
    request: ChatCompletionRequest,
    x_custom_auth: Optional[str] = Header(None)
):
    """Chat completions endpoint compatible with OpenAI API"""

    # Validate authentication if needed
    # if x_custom_auth != os.getenv("CUSTOM_AUTH_TOKEN"):
    #     raise HTTPException(status_code=401, detail="Unauthorized")

    # Validate model
    if not request.model:
        raise HTTPException(status_code=400, detail="Model is required")

    # Validate messages
    if not request.messages:
        raise HTTPException(status_code=400, detail="At least one message is required")

    # Get appropriate LLM provider
    provider = llm_registry.get_provider(request.model)

    # Convert messages format
    messages = [{"role": m.role, "content": m.content} for m in request.messages]

    # Convert tools format
    tools = None
    if request.tools:
        tools = [{"type": "function", "function": {"name": t.function.name, "description": t.function.description, "parameters": t.function.parameters}} for t in request.tools]

    try:
        if request.stream:
            return StreamingResponse(
                stream_response(
                    provider=provider,
                    model=request.model,
                    messages=messages,
                    temperature=request.temperature,
                    max_tokens=request.max_tokens,
                    tools=tools,
                    extra=request.extra
                ),
                media_type="text/event-stream"
            )
        else:
            response_text = await provider.generate(
                messages=messages,
                model=request.model,
                temperature=request.temperature,
                max_tokens=request.max_tokens,
                tools=tools
            )

            return {
                "id": f"chatcmpl-{uuid.uuid4().hex[:12]}",
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
                    "prompt_tokens": estimate_tokens(messages),
                    "completion_tokens": estimate_tokens(response_text),
                    "total_tokens": estimate_tokens(messages) + estimate_tokens(response_text)
                }
            }

    except Exception as e:
        logger.error(f"Error in chat completion: {e}")
        raise HTTPException(status_code=500, detail=f"Internal error: {str(e)}")

async def stream_response(provider, model, messages, temperature, max_tokens, tools, extra):
    """Stream response with proper SSE format"""

    try:
        chunk_id = f"chatcmpl-{uuid.uuid4().hex[:12]}"
        created = int(datetime.now().timestamp())
        first = True

        async for token in provider.stream(
            messages=messages,
            model=model,
            temperature=temperature,
            max_tokens=max_tokens,
            tools=tools
        ):
            delta = {"content": token} if token else {}
            if first:
                delta["role"] = "assistant"
                first = False

            chunk = {
                "id": chunk_id,
                "object": "chat.completion.chunk",
                "created": created,
                "model": model,
                "choices": [{
                    "index": 0,
                    "delta": delta,
                    "finish_reason": None
                }]
            }
            yield f"data: {json.dumps(chunk)}\n\n"

        # Send final chunk
        final = {
            "id": chunk_id,
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "stop"
            }]
        }
        yield f"data: {json.dumps(final)}\n\n"
        yield "data: [DONE]\n\n"

    except Exception as e:
        logger.error(f"Streaming error: {e}")
        error_response = {
            "error": {
                "message": str(e),
                "type": "server_error"
            }
        }
        yield f"data: {json.dumps(error_response)}\n\n"

@app.get("/v1/models")
async def list_models():
    """List available models"""
    return {
        "object": "list",
        "data": [
            {
                "id": "gpt-4",
                "object": "model",
                "created": int(datetime.now().timestamp()),
                "owned_by": "openai"
            },
            {
                "id": "gpt-4-turbo",
                "object": "model",
                "created": int(datetime.now().timestamp()),
                "owned_by": "openai"
            },
            {
                "id": "claude-3-sonnet-20240229",
                "object": "model",
                "created": int(datetime.now().timestamp()),
                "owned_by": "anthropic"
            }
        ]
    }

@app.get("/health")
async def health_check():
    """Health check endpoint"""
    return {"status": "healthy"}

def estimate_tokens(text_or_messages):
    """Rough token estimation (use proper tokenizer in production)"""
    if isinstance(text_or_messages, list):
        return sum(len(str(m.get("content", "")).split()) for m in text_or_messages) * 1.3
    return len(str(text_or_messages).split()) * 1.3

if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="0.0.0.0", port=8000, log_level="info")
```

---

## Part 3: ElevenLabs Integration Guide

### 3.1 Configuration in ElevenLabs Dashboard

```
1. Navigate to your AI Agent settings
2. Go to "Customization" → "Model (LLM)"
3. Click "Change Model"
4. Select "Custom LLM"
5. Fill in:
   - Server URL: https://your-app.ngrok.io
   - Model: gpt-4
   - Create a Secret for authentication (optional)
   - Enable "Custom LLM extra body" for user context
6. Save and test
```

### 3.2 Testing with cURL

```bash
# Test non-streaming
curl -X POST "http://localhost:8000/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4",
    "messages": [
      {"role": "system", "content": "You are a helpful assistant."},
      {"role": "user", "content": "Hello!"}
    ],
    "stream": false
  }'

# Test streaming
curl -X POST "http://localhost:8000/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4",
    "messages": [
      {"role": "system", "content": "You are a helpful assistant."},
      {"role": "user", "content": "Hello!"}
    ],
    "stream": true
  }'

# Test with custom header
curl -X POST "http://localhost:8000/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -H "X-Custom-Auth: my-secret-token" \
  -d '{...}'
```

### 3.3 Python Client Testing

```python
from openai import OpenAI

# Point to your custom LLM server
client = OpenAI(
    api_key="unused",  # Custom endpoint doesn't need real key
    base_url="http://localhost:8000"
)

# Non-streaming
response = client.chat.completions.create(
    model="gpt-4",
    messages=[
        {"role": "system", "content": "You are a helpful assistant."},
        {"role": "user", "content": "What is 2+2?"}
    ]
)
print(response.choices[0].message.content)

# Streaming
stream = client.chat.completions.create(
    model="gpt-4",
    messages=[
        {"role": "system", "content": "You are a helpful assistant."},
        {"role": "user", "content": "What is 2+2?"}
    ],
    stream=True
)

for chunk in stream:
    if chunk.choices[0].delta.content:
        print(chunk.choices[0].delta.content, end="", flush=True)
```

---

## Part 4: Docker Deployment

### 4.1 Dockerfile

```dockerfile
FROM python:3.11-slim

WORKDIR /app

# Install dependencies
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# Copy application
COPY . .

# Expose port
EXPOSE 8000

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
  CMD curl -f http://localhost:8000/health || exit 1

# Run application
CMD ["uvicorn", "app:app", "--host", "0.0.0.0", "--port", "8000"]
```

### 4.2 requirements.txt

```
fastapi==0.104.1
uvicorn==0.24.0
openai==1.3.0
anthropic==0.7.0
pydantic==2.5.0
python-dotenv==1.0.0
```

### 4.3 Docker Compose

```yaml
version: '3.8'

services:
  custom-llm:
    build: .
    ports:
      - "8000:8000"
    environment:
      - OPENAI_API_KEY=${OPENAI_API_KEY}
      - ANTHROPIC_API_KEY=${ANTHROPIC_API_KEY}
    volumes:
      - .:/app
    restart: unless-stopped
```

### 4.4 Build and Run

```bash
# Build image
docker build -t custom-llm .

# Run container
docker run -p 8000:8000 \
  -e OPENAI_API_KEY=$OPENAI_API_KEY \
  custom-llm

# Or use Docker Compose
docker-compose up
```

---

## Part 5: Advanced Patterns

### 5.1 Caching Responses

```python
from functools import lru_cache
import hashlib

@lru_cache(maxsize=1000)
def get_cached_response(model: str, messages_hash: str):
    # Cache implementation
    pass

def hash_messages(messages):
    msg_str = json.dumps(messages, sort_keys=True)
    return hashlib.sha256(msg_str.encode()).hexdigest()
```

### 5.2 Rate Limiting

```python
from slowapi import Limiter
from slowapi.util import get_remote_address

limiter = Limiter(key_func=get_remote_address)
app = FastAPI()

@app.post("/v1/chat/completions")
@limiter.limit("10/minute")
async def chat_completions(request: Request, chat_request: ChatCompletionRequest):
    # Implementation
    pass
```

### 5.3 Request Logging and Monitoring

```python
import logging
from pythonjsonlogger import jsonlogger

logger = logging.getLogger()
handler = logging.StreamHandler()
formatter = jsonlogger.JsonFormatter()
handler.setFormatter(formatter)
logger.addHandler(handler)

@app.post("/v1/chat/completions")
async def chat_completions(request: ChatCompletionRequest):
    logger.info("Chat request", extra={
        "model": request.model,
        "message_count": len(request.messages),
        "stream": request.stream
    })
    # Implementation
```

---

## Part 6: Deployment Options

### 6.1 Heroku

```bash
# Create Procfile
echo "web: uvicorn app:app --host 0.0.0.0 --port \$PORT" > Procfile

# Create runtime.txt
echo "python-3.11.7" > runtime.txt

# Deploy
git push heroku main
```

### 6.2 Railway.app

```bash
# Connect your GitHub repo to Railway
# Add environment variables in Railway dashboard
# Deploy automatically

# Or manually with Railway CLI
railway link
railway up
```

### 6.3 AWS Lambda with Mangum

```python
from mangum import Mangum
from app import app

handler = Mangum(app)
```

### 6.4 Google Cloud Run

```bash
gcloud run deploy custom-llm \
  --source . \
  --platform managed \
  --region us-central1 \
  --allow-unauthenticated
```

---

## Part 7: Monitoring and Debugging

### 7.1 Response Time Monitoring

```python
import time
from fastapi import Request
from starlette.middleware.base import BaseHTTPMiddleware

class TimingMiddleware(BaseHTTPMiddleware):
    async def dispatch(self, request: Request, call_next):
        start = time.time()
        response = await call_next(request)
        duration = time.time() - start
        response.headers["X-Process-Time"] = str(duration)
        return response

app.add_middleware(TimingMiddleware)
```

### 7.2 Logging Requests and Responses

```python
import logging

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

@app.post("/v1/chat/completions")
async def chat_completions(request: ChatCompletionRequest):
    logger.info(f"Request: model={request.model}, messages={len(request.messages)}, stream={request.stream}")

    # Your implementation

    logger.info(f"Response: status=success, tokens={response.get('usage', {}).get('total_tokens', 0)}")
    return response
```

---

## Part 8: Common Issues and Solutions

### 8.1 Streaming Not Working

**Problem:** Client receives complete response instead of stream

**Solution:**
```python
# Ensure proper headers
response = StreamingResponse(
    stream_generator(),
    media_type="text/event-stream",
    headers={
        "Cache-Control": "no-cache",
        "X-Accel-Buffering": "no"
    }
)
```

### 8.2 Connection Timeouts

**Problem:** ElevenLabs times out waiting for response

**Solution:**
```python
# Send initial chunk quickly
async for token in llm_stream():
    yield f"data: {json.dumps(chunk)}\n\n"
    await asyncio.sleep(0)  # Yield control frequently
```

### 8.3 Token Counting Issues

**Problem:** Incorrect token usage reporting

**Solution:**
```python
import tiktoken

def count_tokens_accurate(text: str, model: str = "gpt-4") -> int:
    encoding = tiktoken.encoding_for_model(model)
    return len(encoding.encode(text))
```

---

**Last Updated:** 2026-02-04
**Status:** Complete with production-ready examples
