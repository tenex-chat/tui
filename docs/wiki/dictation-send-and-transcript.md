---
title: Dictation Send & Transcript Handling
slug: dictation-send-and-transcript
summary: The send button is never disabled while dictation is recording, allowing users to always tap to stop and send
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-11
updated: 2026-05-12
verified: 2026-05-11
compiled-from: conversation
sources:
  - session:30510843-67b1-4802-be93-4d0d924f076a
  - session:5b223d53-7743-4a4f-8931-02792480b5a9
  - session:820286f0-ce6c-4fb4-b28b-066b8bc95c17
  - session:d170ed07-181f-444f-8944-f7cf0bd143e8
---

# Dictation Send & Transcript Handling

## Send Button Behavior During Dictation

The send button is never disabled while dictation is recording, allowing users to always tap to stop and send. Tapping the send button while dictation is recording stops the recording instead of calling sendMessage directly, allowing the onChange handler to send the transcribed text and preventing a double-send. When dictation stops and the state transitions to .idle, the .idle handler in MessageComposerView captures the raw dictated text and typed prefix, resets state synchronously, then calls sendDictatedMessage in a Task instead of calling sendMessage() directly. sendMessage() is guarded by canSend so that empty recordings produce a no-op rather than sending an empty message. preDictationText is cleared before sending to prevent double-send. dictationManager.reset() runs before sendMessage() to clean up state prior to sending.

<!-- citations: [^30510-1] [^5b223-1] [^d170e-2] -->
## Mid-Sentence Pause Transcript Preservation

When a user pauses mid-sentence during dictation, the ElevenLabs STT service may silently reset its partial transcript without always sending is_final=true, causing previously transcribed text to be lost. To prevent this, DictationManager tracks currentSegmentText separately from accumulatedText. A committed_transcript message triggers a commit via the isFinal branch, resetting currentSegmentText so a new segment can begin. Upon receiving a partial_transcript message, DictationManager simply replaces the current segment display text, as partial_transcript represents interim text for only the current segment. The hasPrefix heuristic must not be used to synthesize commits from partial transcripts, as it causes duplication when the server revises interim text. When stopRecording() is called, it flushes the current segment text in case recording stops mid-segment before a committed_transcript arrives, preventing dropped words. The dictation display shows accumulatedText + currentSegmentText so that pausing mid-sentence preserves the text spoken before the pause.

<!-- citations: [^30510-2] [^82028-1] -->
## See Also

If VoiceCaptureSheet allows swipe-to-dismiss, the onDismiss handler calling stopRecording() would unintentionally trigger auto-send.

<!-- citations: [^5b223-2] -->
