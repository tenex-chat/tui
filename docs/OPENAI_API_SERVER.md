# OpenAI Responses API Server

TENEX TUI now includes an HTTP server mode that exposes an OpenAI Responses API endpoint for integration with ElevenLabs and other services.

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

The server exposes the endpoint: `POST /:project_dtag/responses`

Replace `:project_dtag` with your project's d-tag identifier (the last part of the project coordinate).

#### Example with curl

```bash
# Simple string input
curl -X POST http://127.0.0.1:3000/my-project/responses \
  -H "Content-Type: application/json" \
  -d '{
    "input": "Hello, what can you help me with?",
    "stream": true
  }'

# Message array input
curl -X POST http://127.0.0.1:3000/my-project/responses \
  -H "Content-Type: application/json" \
  -d '{
    "input": [
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

# Make a streaming request using the Responses API
response = client.responses.create(
    model="tenex",  # Model name is optional
    input="What is TENEX?",
    stream=True
)

for event in response:
    if event.type == "response.output_text.delta":
        print(event.delta, end="")
```

#### Example with JavaScript/TypeScript

```typescript
import OpenAI from 'openai';

const client = new OpenAI({
  baseURL: 'http://127.0.0.1:3000/my-project',
  apiKey: 'not-needed'
});

const stream = await client.responses.create({
  model: 'tenex',
  input: 'Hello!',
  stream: true,
});

for await (const event of stream) {
  if (event.type === 'response.output_text.delta') {
    process.stdout.write(event.delta);
  }
}
```

## Request Format

### Responses Request

```json
{
  "input": "Your message here",
  "stream": true,
  "model": "tenex",
  "previous_response_id": "resp_...",
  "instructions": "System instructions",
  "metadata": {},
  "user": "user-id"
}
```

**Required fields:**
- `input`: Either a simple string or an array of message objects
  - String: `"Your message"`
  - Array: `[{"role": "user", "content": "Your message"}]`
  - Rich content: `[{"role": "user", "content": [{"type": "input_text", "text": "..."}]}]`

**Optional fields:**
- `stream`: Boolean (default: false)
  - `true`: Stream responses via Server-Sent Events (SSE)
  - `false`: Return complete response (not yet implemented)
- `model`: String (optional, TENEX uses its own model)
- `previous_response_id`: String for conversation chaining
- `instructions`: System instructions for the model
- `store`: Boolean to control response storage
- `metadata`: Object with custom metadata
- `user`: User identifier string

### Response Format (Streaming)

Server-Sent Events with OpenAI Responses API format:

```
event: response.created
data: {"type":"response.created","response":{"id":"resp_...","created_at":1234567890.0,"status":"in_progress","model":"tenex","object":"response","output":[]}}

event: response.in_progress
data: {"type":"response.in_progress","response":{"id":"resp_...","status":"in_progress",...}}

event: response.output_item.added
data: {"type":"response.output_item.added","output_index":0,"item":{"id":"msg_...","type":"message","role":"assistant","status":"in_progress","content":[]}}

event: response.content_part.added
data: {"type":"response.content_part.added","output_index":0,"content_index":0,"part":{"type":"output_text","text":"","annotations":[]}}

event: response.output_text.delta
data: {"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":"Hello"}

event: response.output_text.delta
data: {"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":" there!"}

event: response.output_text.done
data: {"type":"response.output_text.done","output_index":0,"content_index":0,"text":"Hello there!"}

event: response.output_item.done
data: {"type":"response.output_item.done","output_index":0,"item":{"id":"msg_...","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Hello there!","annotations":[]}]}}

event: response.completed
data: {"type":"response.completed","response":{"id":"resp_...","status":"completed","output":[...],"output_text":"Hello there!"}}
```

## How It Works

1. **Project Resolution**: The server looks up your project by its d-tag identifier
2. **Agent Selection**: Automatically uses the PM (project manager) agent from the project status
3. **Message Publishing**: Creates a kind:1 Nostr event p-tagging the selected agent
4. **Real-time Streaming**: Forwards agent responses via SSE as they arrive
5. **OpenAI Format**: Converts TENEX streaming format to OpenAI Responses API format

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
2. Configure ElevenLabs to use your endpoint: `http://127.0.0.1:3000/PROJECT_DTAG/responses`
3. ElevenLabs will send user speech as messages and stream agent responses for TTS

## Limitations

- **Non-streaming mode**: Not yet implemented (use `stream: true`)
- **No authentication**: Intended for local/trusted network use only
- **Single project per request**: Project is specified in the URL path
- **Conversation chaining**: Use `previous_response_id` to chain conversations

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
