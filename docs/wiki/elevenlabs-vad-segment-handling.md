---
title: ElevenLabs VAD Segment Handling
slug: elevenlabs-vad-segment-handling
summary: When ElevenLabs commits a VAD segment with is_final=true, the text is often shorter than the running cumulative partial
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-11
updated: 2026-05-12
verified: 2026-05-11
compiled-from: conversation
sources:
  - session:3d0ab623-c663-4728-a8ff-a3a77b91e45a
  - session:a11196a4-22c4-4c28-be5d-bd27c6de1632
  - session:820286f0-ce6c-4fb4-b28b-066b8bc95c17
---

# ElevenLabs VAD Segment Handling

## VAD Final vs. Partial Text

The ElevenLabs WebSocket STT parser must dispatch transcript messages using `message_type` (`partial_transcript` vs `committed_transcript`) rather than a non-existent `is_final` field to correctly set the `isFinal` flag. When ElevenLabs commits a VAD segment, the text is often shorter than the running cumulative partial. The dictation display must prefer the longer partial over the shorter committed text to prevent the displayed text from jumping backwards. The ElevenLabs STT service with commit_strategy=vad silently resets its partial transcript when VAD detects a pause, rather than always sending a committed transcript for the prior utterance. To handle this, the dictation system tracks currentSegmentText separately from accumulatedText to detect when ElevenLabs starts a new VAD segment after a pause. When a new partial transcript arrives that does not extend the current segment text (hasPrefix check), the prior segment text is committed to accumulatedText before continuing with the new segment. The dictation display always shows accumulatedText plus currentSegmentText so that pausing mid-sentence preserves the words spoken before the pause.

<!-- citations: [^3d0ab-1] [^a1119-2] [^82028-2] -->
## See Also

