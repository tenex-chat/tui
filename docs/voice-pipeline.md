# Voice Pipeline

## Objective

Allow users to stay in narrative while the system enforces structure.

## Input Modes

- Primary: iOS microphone capture via `AVAudioEngine`, streamed to ElevenLabs Scribe v2 Realtime.
- Secondary: typed narration fallback for simulator and failure recovery.

No native iOS speech recognition API is part of the product.

## Context Pack

The OpenRouter interpreter receives:

- current date
- current day state
- pending commitments
- flex item
- active threads
- week theme and deliverable
- recent history summary

## Interpreter Contract

The interpreter returns normalized events only.

It must not mutate app state directly.

## Resolver Contract

The resolver:

- validates constraints
- caps commitments and threads
- applies completions
- schedules tomorrow items
- creates or touches threads
- computes redirection and recovery outputs
- appends history events

## Provider Pipeline

1. Create an ElevenLabs `realtime_scribe` single-use token with the saved API key.
2. Capture microphone audio as mono PCM.
3. Stream PCM chunks to ElevenLabs Scribe v2 Realtime over WebSocket using the token query parameter.
4. Display partial transcripts as live text while recording.
5. Accumulate committed segments plus the current partial transcript.
6. Send transcript and context pack to OpenRouter chat completions after stop.
7. Request strict JSON output for interpreter events.
8. Present "What I understood" before applying anything.
9. Apply only confirmed events.

## API Endpoints

ElevenLabs Speech to Text:

```txt
POST https://api.elevenlabs.io/v1/single-use-token/realtime_scribe
WSS wss://api.elevenlabs.io/v1/speech-to-text/realtime
```

Required connection/query values:

- `xi-api-key` header on the token request
- `token=<single-use-token>` on the WebSocket request
- `model_id=scribe_v2_realtime`
- `audio_format=pcm_16000`
- `commit_strategy=vad`

Runtime messages:

- send `input_audio_chunk` with base64 PCM audio
- receive `partial_transcript` for live text
- receive `committed_transcript` for finalized VAD segments

OpenRouter reasoning:

```txt
POST https://openrouter.ai/api/v1/chat/completions
```

Required behavior:

- include `response_format` with JSON schema
- normalize `choices[0].message.content`

ElevenLabs Text to Speech:

```txt
POST https://api.elevenlabs.io/v1/text-to-speech/:voice_id
```

Used only for optional spoken recovery or next-action prompts.
