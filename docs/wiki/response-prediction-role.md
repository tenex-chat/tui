---
title: Response Prediction Role
slug: response-prediction-role
summary: The ResponsePrediction role is defined in the OpenRouterModelRole enum with raw value "response_prediction", display title "Response prediction", description "P
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-11
updated: 2026-05-11
verified: 2026-05-11
compiled-from: conversation
sources:
  - session:60f6b258-8aa5-45cc-b1bd-dd35ebf37fe7
---

# Response Prediction Role

## Response Prediction Role

The ResponsePrediction role is defined in the OpenRouterModelRole enum with raw value "response_prediction", display title "Response prediction", description "Predicts what you might want to say next", and SF Symbol "text.bubble". The Settings UI displays the ResponsePrediction model role configuration because AppSettingsView iterates over OpenRouterModelRole.allCases. However, no code in the app calls selectedModel(for: .responsePrediction) to use the configured model — the feature is defined in the codec and settings infrastructure but not implemented. [^60f6b-1]

## See Also

