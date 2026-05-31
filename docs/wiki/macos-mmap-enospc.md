---
title: macOS mmap ENOSPC on Swap Exhaustion
slug: macos-mmap-enospc
summary: On macOS, `mmap` returns `ENOSPC` when swap is exhausted, preventing LMDB from opening even if physical disk space exists.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-23
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:22ca3a53-7ac3-4b05-8469-cf0a16e53ede
  - session:7fd77178-a7b9-47dd-931c-0298d0bb153e
---

# macOS mmap ENOSPC on Swap Exhaustion

## macOS mmap ENOSPC

On macOS, `mmap` returns `ENOSPC` when swap is exhausted, preventing LMDB from opening even if physical disk space exists. When `mdb_env_open` fails with `ENOSPC`, nostrdb's `ndb_init_lmdb` destroys and recreates the LMDB environment, then retries with the `MDB_NOLOCK` flag to bypass macOS broken semaphores. The `ENOSPC` retry with `MDB_NOLOCK` is safe for the TUI because it has single-writer semantics. Note that `nostrdb.c` requires `#include <errno.h>` for the `ENOSPC` constant to compile. Additionally, the patch to `nostrdb.c` must be applied in the cargo git cache, which means it will be lost on `cargo clean` or fresh dependency fetch unless the nostrdb-rs repo is forked.

<!-- citations: [^22ca3-1] [^7fd77-1] -->
## See Also

