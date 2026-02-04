# OpenAI-Compatible API Server

TENEX TUI now includes an HTTP server mode that exposes an OpenAI-compatible API endpoint for integration with ElevenLabs and other services.

## Quick Start

### Starting the Server

```bash
# Using nsec from environment variable (recommended)
export TENEX_NSEC="nsec1..."
tenex-tui --server

# Or pass nsec directly (not recommended - exposes key in shell history)
tenex-tui --server --nsec nsec1...

# Custom bind address
tenex-tui --server --bind 0.0.0.0:8080
```

Default server address: `http://127.0.0.1:3000`

### Making Requests

The server exposes the endpoint: `POST /:project_dtag/chat/completions`

Replace `:project_dtag` with your project's d-tag identifier (the last part of the project coordinate).

#### Example with curl

```bash
curl -X POST http://127.0.0.1:3000/my-project/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "messages": [
      {"role": "user", "content": "Hello, what can you help me with?"}
    ],
    "stream": true
  }'
```

#### Example with OpenAI Python SDK

```python
from openai import OpenAI

# Point the client to your local TENEX server
client = OpenAI(
    base_url="http://127.0.0.1:3000/my-project",
    api_key="not-needed"  # No authentication required
)

# Make a streaming request
stream = client.chat.completions.create(
    model="tenex",  # Model name is ignored but required
    messages=[
        {"role": "user", "content": "What is TENEX?"}
    ],
    stream=True
)

for chunk in stream:
    if chunk.choices[0].delta.content:
        print(chunk.choices[0].delta.content, end="")
```

#### Example with JavaScript/TypeScript

```typescript
import OpenAI from 'openai';

const client = new OpenAI({
  baseURL: 'http://127.0.0.1:3000/my-project',
  apiKey: 'not-needed'
});

const stream = await client.chat.completions.create({
  model: 'tenex',
  messages: [{ role: 'user', content: 'Hello!' }],
  stream: true,
});

for await (const chunk of stream) {
  process.stdout.write(chunk.choices[0]?.delta?.content || '');
}
```

## Request Format

### Chat Completion Request

```json
{
  "messages": [
    {"role": "user", "content": "Your message here"}
  ],
  "stream": true,
  "model": "tenex"
}
```

**Required fields:**
- `messages`: Array of message objects with `role` and `content`
  - At least one message with `role: "user"` is required
  - The last user message is sent to the agent

**Optional fields:**
- `stream`: Boolean (default: false)
  - `true`: Stream responses via Server-Sent Events (SSE)
  - `false`: Return complete response (not yet implemented)
- `model`: String (ignored but may be required by some clients)

### Response Format (Streaming)

Server-Sent Events with OpenAI-compatible format:

```
data: {"id":"api-...","object":"chat.completion.chunk","created":1234567890,"model":"tenex","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}

data: {"id":"api-...","object":"chat.completion.chunk","created":1234567890,"model":"tenex","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}

data: {"id":"api-...","object":"chat.completion.chunk","created":1234567890,"model":"tenex","choices":[{"index":0,"delta":{"content":" there"},"finish_reason":null}]}

data: {"id":"api-...","object":"chat.completion.chunk","created":1234567890,"model":"tenex","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}

data: [DONE]
```

## How It Works

1. **Project Resolution**: The server looks up your project by its d-tag identifier
2. **Agent Selection**: Automatically uses the PM (project manager) agent from the project status
3. **Message Publishing**: Creates a kind:1 Nostr event p-tagging the selected agent
4. **Real-time Streaming**: Forwards agent responses via SSE as they arrive
5. **OpenAI Format**: Converts TENEX streaming format to OpenAI-compatible chunks

## Architecture

- **Framework**: Axum (Tokio-native HTTP server)
- **Streaming**: Server-Sent Events (SSE)
- **Event System**: Reuses existing `CoreRuntime` and `DataChange` channel
- **No Auth**: Designed for local/trusted network use

## Configuration

### Command Line Flags

```
--server          Run in HTTP server mode (instead of TUI)
--bind <ADDR>     Server bind address (default: 127.0.0.1:3000)
--nsec <NSEC>     Nostr secret key (prefer TENEX_NSEC env var)
```

### Environment Variables

- `TENEX_NSEC`: Your Nostr secret key (nsec format)
- `TENEX_DEBUG=1`: Enable debug logging

## Integration with ElevenLabs

The primary use case is integrating ElevenLabs conversational AI with TENEX agents:

1. Start TENEX server: `TENEX_NSEC=nsec1... tenex-tui --server`
2. Configure ElevenLabs to use your endpoint: `http://127.0.0.1:3000/PROJECT_DTAG/chat/completions`
3. ElevenLabs will send user speech as messages and stream agent responses for TTS

## Limitations

- **Non-streaming mode**: Not yet implemented (use `stream: true`)
- **No authentication**: Intended for local/trusted network use only
- **Single project per request**: Project is specified in the URL path
- **No conversation history**: Each request is independent (new thread ID)

## Troubleshooting

### "Project not found" Error

- Verify your project d-tag is correct (last part of the coordinate)
- Ensure the TENEX client is connected and has received project data
- Check that the project is visible in the TUI mode

### "Agent not available" Error

- Project status event must include at least one agent
- The PM agent must be online (recent kind:24010 event)
- Check project status in the diagnostics view

### Connection Issues

- Verify the server is running: `curl http://127.0.0.1:3000/health` (returns 404 but confirms server is up)
- Check firewall settings if binding to non-localhost address
- Ensure port 3000 is not already in use

## Security Notes

⚠️ **Important**: This server has no authentication mechanism. It is designed for:
- Local development
- Trusted networks
- Single-user environments

Do not expose this server to the public internet without adding authentication.

## Examples

See the [examples directory](../examples/) for complete working examples:
- `openai_client.py`: Python client using OpenAI SDK
- `curl_examples.sh`: Shell script with curl examples
- `elevenlabs_integration.md`: ElevenLabs setup guide

## Future Enhancements

Potential improvements for future versions:
- Non-streaming response mode
- Authentication/API keys
- Multiple concurrent conversations
- Conversation history management
- WebSocket support
- Rate limiting
- Metrics/monitoring endpoint
