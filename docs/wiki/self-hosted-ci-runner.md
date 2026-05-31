---
title: Self-Hosted CI Runner
slug: self-hosted-ci-runner
summary: The GitHub Actions workflow triggers on push to master and runs on a self-hosted runner, using the same self-hosted runner configuration as the win-the-day app
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-07
updated: 2026-05-13
verified: 2026-05-07
compiled-from: conversation
sources:
  - session:173aa11a-834f-405c-8af8-c8a45f409020
  - session:d18381a0-39d5-4141-be58-03362b5bd636
  - session:acaa32eb-b3b5-4a83-9aee-822648c76ca7
  - session:fbd9a382-9ebb-450e-a699-37ca3f63241c
---

# Self-Hosted CI Runner

## Self-Hosted Runner Configuration

The GitHub Actions workflow triggers on push to master and runs on a self-hosted runner, using the same self-hosted runner configuration as the win-the-day app. The self-hosted GitHub Actions runner for TENEX is installed at `~/actions-runner-tenex` and runs as a launchd service via `svc.sh install` and `svc.sh start`. TestFlight deploys are triggered by pushing to the master branch via GitHub Actions CI workflow, but the TestFlight deployment itself runs directly on the local machine without using the self-hosted GitHub Actions runner (which is registered to a different repository).

<!-- citations: [^173aa-3] [^d1838-2] [^acaa3-2] [^fbd9a-3] -->
## See Also

