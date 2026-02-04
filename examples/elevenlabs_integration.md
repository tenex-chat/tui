# ElevenLabs Integration with TENEX

This guide shows how to integrate ElevenLabs Conversational AI with TENEX agents using the OpenAI-compatible API server.

## Overview

ElevenLabs provides conversational AI that can:
- Convert speech to text (user input)
- Send messages to a language model
- Convert text responses to speech (TTS)
- Maintain natural conversation flow

By pointing ElevenLabs at the TENEX API server, you can have voice conversations with your TENEX agents.

## Setup Steps

### 1. Start TENEX Server

```bash
export TENEX_NSEC="nsec1..."
tenex-tui --server --bind 0.0.0.0:3000
```

Note: Use `0.0.0.0` if ElevenLabs needs to access the server from another machine.

### 2. Get Your Project D-Tag

The project d-tag is the identifier (last part) of your project coordinate.

For example, if your project coordinate is:
```
31933:abc123...def:my-tenex-project
```

Your d-tag is: `my-tenex-project`

### 3. Configure ElevenLabs

1. Log in to [ElevenLabs Dashboard](https://elevenlabs.io/)
2. Go to Conversational AI settings
3. Select "Custom LLM" or "OpenAI-compatible API"
4. Configure the endpoint:

```
Base URL: http://YOUR_SERVER_IP:3000/YOUR_PROJECT_DTAG
API Key: not-needed
```

For example:
```
Base URL: http://192.168.1.100:3000/my-tenex-project
API Key: not-needed
```

### 4. Configure System Prompt (Optional)

You can set a system prompt in ElevenLabs that will be included in the messages array. TENEX will use the last user message.

### 5. Test the Integration

1. Start a conversation in ElevenLabs
2. Speak to test the voice input
3. The agent's response should be streamed back and converted to speech

## Architecture

```
User Speech
    ↓
ElevenLabs (Speech-to-Text)
    ↓
HTTP POST /project/chat/completions
    ↓
TENEX Server
    ↓
Nostr (kind:1 event with p-tag)
    ↓
TENEX Agent
    ↓
Nostr streaming response
    ↓
TENEX Server (SSE)
    ↓
ElevenLabs (Text-to-Speech)
    ↓
User hears response
```

## Message Flow

1. **User speaks**: ElevenLabs captures audio
2. **STT**: ElevenLabs converts to text
3. **API Request**: ElevenLabs sends POST request:
   ```json
   {
     "messages": [
       {"role": "user", "content": "transcribed text"}
     ],
     "stream": true
   }
   ```
4. **TENEX Processing**:
   - Resolves project by d-tag
   - Gets PM agent from project status
   - Creates kind:1 Nostr event
   - P-tags the agent
5. **Agent Response**: Agent processes and streams response
6. **SSE Streaming**: TENEX forwards chunks to ElevenLabs
7. **TTS**: ElevenLabs converts to speech as chunks arrive
8. **User hears**: Natural conversation continues

## Advanced Configuration

### Multiple Agents

By default, TENEX uses the PM (project manager) agent. To use a different agent, you would need to modify the server code or create separate endpoints.

### Conversation Context

Each request creates a new thread ID. For conversation history, ElevenLabs maintains the context and sends it in the `messages` array.

### Custom Models

ElevenLabs may send a `model` field. TENEX ignores this field but requires it for OpenAI SDK compatibility.

## Troubleshooting

### "Project not found" Error

**Problem**: ElevenLabs receives 404 error

**Solutions**:
- Verify project d-tag is correct
- Check TENEX server logs
- Ensure project is loaded in TENEX (run TUI mode first to load projects)

### "Agent not available" Error

**Problem**: ElevenLabs receives 503 error

**Solutions**:
- Check project status has online agents
- Verify kind:24010 events are being received
- Check agent is not stale (STALENESS_THRESHOLD_SECS)

### Slow Response Times

**Problem**: Long delay before speech output

**Solutions**:
- Check network latency
- Verify TENEX server is on fast network
- Consider local deployment of all components
- Check Nostr relay connection quality

### Audio Cuts Off

**Problem**: Agent's response is interrupted

**Solutions**:
- Verify SSE stream stays open
- Check for network interruptions
- Ensure ElevenLabs timeout settings are appropriate
- Monitor TENEX server logs for errors

### No Audio Output

**Problem**: Request succeeds but no speech

**Solutions**:
- Verify ElevenLabs TTS is configured
- Check ElevenLabs voice selection
- Test with curl to verify server is returning data
- Check browser/app audio permissions

## Example Request from ElevenLabs

This is what ElevenLabs typically sends:

```json
POST /my-project/chat/completions
Content-Type: application/json

{
  "model": "gpt-4",
  "messages": [
    {
      "role": "system",
      "content": "You are a helpful AI assistant."
    },
    {
      "role": "user",
      "content": "Hello, can you help me?"
    }
  ],
  "stream": true,
  "temperature": 0.7,
  "max_tokens": 2048
}
```

TENEX extracts the last user message and forwards it to the agent.

## Example SSE Response

This is what TENEX sends back to ElevenLabs:

```
data: {"id":"api-123","object":"chat.completion.chunk","created":1234567890,"model":"tenex","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}

data: {"id":"api-123","object":"chat.completion.chunk","created":1234567890,"model":"tenex","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}

data: {"id":"api-123","object":"chat.completion.chunk","created":1234567890,"model":"tenex","choices":[{"index":0,"delta":{"content":"!"},"finish_reason":null}]}

data: {"id":"api-123","object":"chat.completion.chunk","created":1234567890,"model":"tenex","choices":[{"index":0,"delta":{"content":" I"},"finish_reason":null}]}

data: {"id":"api-123","object":"chat.completion.chunk","created":1234567890,"model":"tenex","choices":[{"index":0,"delta":{"content":"'d"},"finish_reason":null}]}

data: {"id":"api-123","object":"chat.completion.chunk","created":1234567890,"model":"tenex","choices":[{"index":0,"delta":{"content":" be"},"finish_reason":null}]}

data: {"id":"api-123","object":"chat.completion.chunk","created":1234567890,"model":"tenex","choices":[{"index":0,"delta":{"content":" happy"},"finish_reason":null}]}

data: {"id":"api-123","object":"chat.completion.chunk","created":1234567890,"model":"tenex","choices":[{"index":0,"delta":{"content":" to"},"finish_reason":null}]}

data: {"id":"api-123","object":"chat.completion.chunk","created":1234567890,"model":"tenex","choices":[{"index":0,"delta":{"content":" help"},"finish_reason":null}]}

data: {"id":"api-123","object":"chat.completion.chunk","created":1234567890,"model":"tenex","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}

data: [DONE]
```

## Performance Tips

1. **Local Deployment**: Run TENEX server on same machine or LAN as ElevenLabs
2. **Fast Relay**: Use a fast Nostr relay with low latency
3. **Optimize Agent**: Configure agent for quick response times
4. **Network**: Use wired connection for best performance
5. **Monitor**: Watch TENEX logs for bottlenecks

## Security Considerations

⚠️ **Important**: The TENEX server has no authentication!

**For ElevenLabs Integration**:
- Use on trusted networks only
- Consider VPN for remote access
- Do not expose server to public internet
- Monitor access logs
- Use firewall rules to restrict access

**Future**: Authentication will be added in a future release.

## Next Steps

- Test with simple queries first
- Monitor server logs for issues
- Experiment with different agents
- Provide feedback for improvements

## Support

For issues or questions:
- Check TENEX server logs
- Review ElevenLabs documentation
- Test with curl to isolate issues
- Check network connectivity
