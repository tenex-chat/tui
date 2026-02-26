# ElevenLabs Integration with TENEX

This guide shows how to integrate ElevenLabs Conversational AI with TENEX agents using the OpenAI Responses API server.

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

You can set a system prompt in ElevenLabs using the `instructions` field. TENEX will use the last user message from the input.

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
HTTP POST /project/responses
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
     "input": "transcribed text",
     "stream": true
   }
   ```
   Or with message array:
   ```json
   {
     "input": [
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

For conversation continuity, use `previous_response_id` to chain responses:
```json
{
  "input": "Follow up question",
  "previous_response_id": "resp_abc123...",
  "stream": true
}
```

### Custom Models

ElevenLabs may send a `model` field. TENEX accepts but ignores this field.

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

This is what ElevenLabs typically sends using the Responses API:

```json
POST /my-project/responses
Content-Type: application/json

{
  "model": "gpt-4",
  "input": "Hello, can you help me?",
  "instructions": "You are a helpful AI assistant.",
  "stream": true
}
```

Or with message array format:

```json
POST /my-project/responses
Content-Type: application/json

{
  "model": "gpt-4",
  "input": [
    {"role": "user", "content": "Hello, can you help me?"}
  ],
  "instructions": "You are a helpful AI assistant.",
  "stream": true
}
```

TENEX extracts the user content and forwards it to the agent.

## Example SSE Response

This is what TENEX sends back to ElevenLabs using the Responses API format:

```
event: response.created
data: {"type":"response.created","response":{"id":"resp_abc123","created_at":1234567890.0,"status":"in_progress","model":"tenex","object":"response","output":[]}}

event: response.in_progress
data: {"type":"response.in_progress","response":{"id":"resp_abc123","status":"in_progress",...}}

event: response.output_item.added
data: {"type":"response.output_item.added","output_index":0,"item":{"id":"msg_xyz789","type":"message","role":"assistant","status":"in_progress","content":[]}}

event: response.content_part.added
data: {"type":"response.content_part.added","output_index":0,"content_index":0,"part":{"type":"output_text","text":"","annotations":[]}}

event: response.output_text.delta
data: {"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":"Hello"}

event: response.output_text.delta
data: {"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":"!"}

event: response.output_text.delta
data: {"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":" I'd be happy to help!"}

event: response.output_text.done
data: {"type":"response.output_text.done","output_index":0,"content_index":0,"text":"Hello! I'd be happy to help!"}

event: response.output_item.done
data: {"type":"response.output_item.done","output_index":0,"item":{"id":"msg_xyz789","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Hello! I'd be happy to help!","annotations":[]}]}}

event: response.completed
data: {"type":"response.completed","response":{"id":"resp_abc123","status":"completed","output":[...],"output_text":"Hello! I'd be happy to help!"}}
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
