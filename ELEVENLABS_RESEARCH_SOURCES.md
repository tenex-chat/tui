# Research Sources - ElevenLabs Custom LLM API Specification

**Research Date:** February 4, 2026
**Total Sources Researched:** 25+ official documentation and reference pages

This document lists all sources used to compile the comprehensive ElevenLabs Custom LLM API research.

---

## Official ElevenLabs Documentation

### Primary Documentation
- **Custom LLM Integration Guide** - https://elevenlabs.io/docs/agents-platform/customization/llm/custom-llm
  - Core specification for custom LLM endpoints
  - Configuration requirements
  - Authentication setup

- **ElevenLabs API Reference** - https://elevenlabs.io/docs/api-reference/introduction
  - Base API documentation
  - Standard authentication patterns
  - API capabilities overview

- **ElevenLabs Agents Platform Overview** - https://elevenlabs.io/docs/agents-platform/overview
  - Platform architecture
  - Agent capabilities
  - Integration options

### LLM Model Documentation
- **Models** - https://elevenlabs.io/docs/agents-platform/customization/llm
  - Available LLM options
  - Model-specific configurations
  - LLM cascading features

- **LLM Cascading** - https://elevenlabs.io/docs/agents-platform/customization/llm/llm-cascading
  - Fallback behavior
  - Error handling
  - Model selection logic

### TTS and Streaming
- **Stream speech** - https://elevenlabs.io/docs/api-reference/text-to-speech/stream
  - HTTP streaming API specification
  - Real-time TTS features
  - Audio format details

- **WebSocket** - https://elevenlabs.io/docs/api-reference/text-to-speech/v-1-text-to-speech-voice-id-stream-input
  - WebSocket TTS streaming
  - Real-time bidirectional communication
  - Voice customization

- **Streaming (General)** - https://elevenlabs.io/docs/api-reference/streaming
  - Streaming architecture
  - Multi-context support
  - Performance characteristics

### Agent Configuration
- **Authentication** - https://elevenlabs.io/docs/api-reference/authentication
  - API key authentication
  - Secret management
  - Custom authentication

- **Agent Authentication** - https://elevenlabs.io/docs/agents-platform/customization/authentication
  - Custom LLM authentication
  - Header configuration
  - Secret storage

- **System Tools** - https://elevenlabs.io/docs/agents-platform/customization/tools/system-tools
  - Tool/function definition format
  - Tool calling mechanism
  - Integration with custom LLMs

- **Server Tools** - https://elevenlabs.io/docs/agents-platform/customization/tools/server-tools
  - External tool endpoints
  - Tool execution context
  - Response handling

### Personalization and Context
- **Overrides** - https://elevenlabs.io/docs/agents-platform/customization/personalization/overrides
  - Runtime behavior customization
  - Per-conversation configuration
  - Security settings

- **Dynamic Variables** - https://elevenlabs.io/docs/agents-platform/customization/personalization/dynamic-variables
  - Runtime value injection
  - User personalization
  - System prompt customization

### Conversation and Simulation
- **Simulate Conversation** - https://elevenlabs.io/docs/api-reference/agents/simulate-conversation
  - Conversation API endpoint
  - Request format
  - Response structure

- **Stream Simulate Conversation** - https://elevenlabs.io/docs/api-reference/agents/simulate-conversation-stream
  - Streaming conversation API
  - Incremental response handling
  - Stream format specification

- **Simulate Conversations (Guide)** - https://elevenlabs.io/docs/agents-platform/guides/simulate-conversations
  - Testing agents
  - Conversation simulation patterns
  - Best practices

### Webhooks and Post-Call Processing
- **Post-call Webhooks** - https://elevenlabs.io/docs/agents-platform/workflows/post-call-webhooks
  - Webhook payload format
  - Post-call data delivery
  - Authentication and validation

### Additional Integration Guides
- **Quickstart** - https://elevenlabs.io/docs/agents-platform/quickstart
  - Getting started guide
  - Basic setup steps
  - Initial configuration

- **Integrating external agents** - https://elevenlabs.io/blog/integrating-complex-external-agents
  - Complex orchestrator integration
  - Agentic behavior support
  - Advanced patterns

### Provider-Specific Guides
- **Together AI Integration** - https://elevenlabs.io/docs/agents-platform/customization/llm/custom-llm/together-ai
  - Custom LLM provider example
  - Configuration specifics
  - Authentication setup

- **Groq Cloud Integration** - https://elevenlabs.io/docs/agents-platform/customization/llm/custom-llm/groq-cloud
  - Alternative provider example
  - API-specific configuration
  - Performance characteristics

- **SambaNova Cloud Integration** - https://elevenlabs.io/docs/agents-platform/customization/llm/custom-llm/samba-nova-cloud
  - Enterprise provider example
  - Advanced configuration
  - Optimization features

- **Cloudflare Workers AI Integration** - https://elevenlabs.io/docs/agents-platform/customization/llm/custom-llm/cloudflare
  - Edge computing example
  - Serverless deployment
  - Cost optimization

---

## OpenAI API Documentation

### Chat Completions API
- **Chat Completions API Reference** - https://platform.openai.com/docs/api-reference/chat
  - Request format specification
  - Response format specification
  - Parameter documentation

### Models
- **Models List** - https://platform.openai.com/docs/api-reference/models/list
  - `/v1/models` endpoint specification
  - Model listing format
  - Available model information

### Streaming
- **Streaming** - https://platform.openai.com/docs/api-reference/chat-streaming
  - Streaming response format
  - SSE specification
  - Chunk structure

- **Streaming Responses (Guide)** - https://platform.openai.com/docs/guides/streaming-responses
  - Implementation guide
  - Best practices
  - Performance optimization

### Advanced Features
- **Structured Model Outputs** - https://platform.openai.com/docs/guides/structured-outputs
  - JSON schema support
  - Response validation
  - Format constraints

- **Function Calling** - https://platform.openai.com/docs/guides/function-calling
  - Tool/function definition format
  - Function calling flow
  - Response handling

- **Responses API** - https://platform.openai.com/docs/api-reference/responses
  - Newer API format
  - Streaming events
  - Advanced features

---

## GitHub Resources

### Official ElevenLabs Documentation Repository
- **elevenlabs-docs repository** - https://github.com/elevenlabs/elevenlabs-docs
  - Source documentation files
  - Custom LLM overview documentation
  - Integration examples

### ElevenLabs Python SDK
- **elevenlabs-python Issues** - https://github.com/elevenlabs/elevenlabs-python/issues/597
  - Real-world integration issues
  - Configuration troubleshooting
  - SDK usage examples

### Reference Implementations
- **elevenlabs-zep-example** (mentioned in documentation)
  - Custom LLM proxy implementation
  - Zep integration example
  - Best practices demonstration

### Third-Party Tools and Libraries
- **LiteLLM** - https://github.com/BerriAI/litellm
  - Multi-provider LLM proxy
  - OpenAI compatibility layer
  - Production-ready implementation

- **lm-proxy** - https://github.com/Nayjest/lm-proxy
  - Lightweight OpenAI-compatible proxy
  - FastAPI-based implementation
  - Provider-agnostic design

---

## Third-Party Frameworks and Tools

### Real-Time AI Frameworks
- **Pipecat** - https://docs.pipecat.ai/server/services/tts/elevenlabs
  - Real-time AI framework
  - ElevenLabs TTS integration
  - WebSocket streaming patterns

- **LiveKit Agents** - https://docs.livekit.io/agents/models/tts/plugins/elevenlabs/
  - Agent framework with ElevenLabs
  - Plugin architecture
  - Voice agent patterns

### Production Tools
- **Zep Documentation** - https://help.getzep.com/elevenlabs
  - Memory management for agents
  - ElevenLabs integration
  - Multi-turn conversation handling

- **liteLLM Documentation** - https://docs.litellm.ai/docs/providers/elevenlabs
  - Provider-specific configuration
  - API compatibility
  - Advanced features

- **Mem0 Integration** - https://docs.mem0.ai/integrations/elevenlabs
  - Memory and context management
  - ElevenLabs integration
  - Personalization patterns

---

## Blog Posts and Articles

### ElevenLabs Blog
- **Integrating external agents** - https://elevenlabs.io/blog/integrating-complex-external-agents
  - Real-world integration patterns
  - Complex agentic behavior
  - Best practices

- **Latency optimization** - https://elevenlabs.io/blog/how-do-you-optimize-latency-for-conversational-ai/
  - Performance optimization techniques
  - Buffer word strategy
  - TTS optimization

- **Conversational AI latency** - https://elevenlabs.io/blog/enhancing-conversational-ai-latency-with-efficient-tts-pipelines
  - Architecture optimization
  - Pipeline efficiency
  - Real-time requirements

### Third-Party Articles
- **Medium: Create Your Own AI Voice Assistant** - https://medium.com/@gagancopvtlimited/create-your-own-ai-voice-assistant-that-knows-everything-about-you-with-elevenlabs-b4baf4745464
  - Practical implementation guide (January 2026)
  - Ollama integration
  - Step-by-step setup

- **How to Stream OpenAI Chat Completions** - https://complereinfosystem.com/blogs/stream-openai-chat-completion
  - Streaming implementation guide
  - SSE format details
  - Code examples

- **Stream OpenAI responses from functions** - https://www.openfaas.com/blog/openai-streaming-responses/
  - Serverless function streaming
  - SSE implementation
  - Production patterns

- **OpenAI SSE Streaming API** - https://betterprogramming.pub/openai-sse-sever-side-events-streaming-api-733b8ec32897
  - SSE protocol deep dive
  - Event format specification
  - Implementation details

---

## API Documentation Platforms

### Postman Collections
- **ElevenLabs API Documentation** - https://www.postman.com/elevenlabs/elevenlabs/collection/7i9rytu/elevenlabs-api-documentation
  - Interactive API reference
  - Example requests/responses
  - Authentication examples

### Platform-Specific Documentation
- **Azure OpenAI** - https://learn.microsoft.com/en-us/azure/ai-foundry/openai/
  - OpenAI API variants
  - Authentication differences
  - Compatibility notes

- **OpenVINO Documentation** - https://docs.openvino.ai/2025/model-server/ovms_docs_rest_api_chat.html
  - OpenAI API compatibility
  - REST API specification
  - Server implementation

---

## Help Centers and Support

### ElevenLabs Support
- **API Key Authorization** - https://help.elevenlabs.io/hc/en-us/articles/14599447207697-How-do-I-authorize-myself-using-an-API-key
  - Authentication guide
  - API key setup
  - Common issues

- **What can I create with ElevenLabs Agents** - https://help.elevenlabs.io/hc/en-us/articles/29297893189137-What-can-I-create-with-ElevenLabs-Agents-formerly-Conversational-AI
  - Feature overview
  - Capabilities
  - Use cases

### Community Forums
- **OpenAI Developer Community** (multiple threads)
  - Chat completions implementation
  - Streaming best practices
  - Troubleshooting discussions

- **n8n Community** - ElevenLabs Agents discussion
  - Integration patterns
  - Webhook configuration
  - Common issues

---

## Implementation Examples and Tools

### vLLM
- **OpenAI-Compatible Server** - https://docs.vllm.ai/en/stable/serving/openai_compatible_server/
  - OpenAI API compatibility
  - Server implementation
  - Performance optimization

### FastAPI and Deployment
- **FastAPI Documentation** (referenced throughout)
  - Web framework for implementations
  - Async programming patterns
  - Streaming response handling

### ngrok
- **ngrok Documentation** - https://ngrok.com/
  - Tunneling for local development
  - Authentication setup
  - Production domains

### Cloud Deployment
- **Railway.app** (referenced as deployment option)
- **Heroku** (referenced as deployment option)
- **AWS Lambda** (referenced as deployment option)
- **Google Cloud Run** (referenced as deployment option)

---

## Research Coverage Summary

### API Specifications Covered
- ✓ ElevenLabs custom LLM endpoint requirements
- ✓ OpenAI /v1/chat/completions specification
- ✓ OpenAI /v1/models endpoint
- ✓ Request payload structure and validation
- ✓ Response format (streaming and non-streaming)
- ✓ Server-Sent Events (SSE) format
- ✓ Tool/function calling specification
- ✓ Authentication mechanisms
- ✓ Error handling and status codes
- ✓ Rate limiting and quotas

### Implementation Topics Covered
- ✓ FastAPI server implementation
- ✓ OpenAI client integration
- ✓ Anthropic Claude integration
- ✓ Multi-provider abstraction
- ✓ Streaming implementation
- ✓ Error handling and logging
- ✓ Performance optimization
- ✓ Security and authentication
- ✓ Deployment strategies
- ✓ Monitoring and debugging

### ElevenLabs Integration Covered
- ✓ Custom LLM configuration
- ✓ Tool/function calling
- ✓ System prompts and context
- ✓ Dynamic variables
- ✓ Conversation overrides
- ✓ TTS integration
- ✓ WebSocket streaming
- ✓ Buffer words optimization
- ✓ Post-call webhooks
- ✓ Authentication setup

---

## Quality Assurance

### Information Sources
- **Official Documentation:** 20+ official ElevenLabs and OpenAI documentation pages
- **GitHub References:** 4+ official repositories and implementations
- **Third-Party Integration:** 5+ production tools and frameworks
- **Blog and Articles:** 4+ technical blog posts from official and reputable sources
- **API Documentation Platforms:** 2+ standardized API documentation platforms
- **Community Resources:** Multiple community discussions and Q&A forums

### Verification Methods
- All major specifications cross-referenced with official documentation
- Multiple implementation examples provided
- Real-world integration patterns included
- Current as of February 2026
- Practical testing examples included

### Currency
- Latest ElevenLabs documentation (2026)
- Current OpenAI API specifications
- Recent third-party integration guides
- Modern Python/FastAPI best practices
- Current deployment platforms and tools

---

## Note on Sources

All sources listed above are:
1. **Publicly accessible** - No paywalled or restricted content
2. **Official or reputable** - From official organizations or established technical sources
3. **Current** - Updated as of early 2026
4. **Comprehensive** - Cover specification, implementation, and best practices
5. **Practical** - Include examples and real-world usage patterns

This research synthesizes information from these sources to provide a comprehensive, practical guide for implementing custom LLM endpoints compatible with ElevenLabs Agents Platform.

---

## Documentation Cross-References

The research documents use information from these sources:

- **ELEVENLABS_QUICK_START.md**
  - Primary: ElevenLabs Custom LLM Guide
  - Secondary: OpenAI Chat Completions API
  - Tertiary: FastAPI documentation

- **ELEVENLABS_API_RESEARCH.md**
  - Primary: All official ElevenLabs documentation
  - Secondary: All OpenAI API documentation
  - Tertiary: Third-party integration guides

- **ELEVENLABS_IMPLEMENTATION_EXAMPLES.md**
  - Primary: FastAPI + OpenAI/Anthropic documentation
  - Secondary: Third-party tools (LiteLLM, vLLM)
  - Tertiary: Deployment platform documentation

---

**Research Completed:** February 4, 2026
**Total Source Pages:** 25+
**Status:** Comprehensive and Current
