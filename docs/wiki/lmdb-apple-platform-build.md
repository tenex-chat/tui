---
title: LMDB Apple Platform Build Flags
slug: lmdb-apple-platform-build
summary: On Apple targets, LMDB uses pthread process-shared mutexes instead of POSIX named semaphores, controlled by the `MDB_SKIP_POSIX_SEM` build flag
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-25
updated: 2026-05-25
verified: 2026-05-25
compiled-from: conversation
sources:
  - session:8835ccc3-ba38-4ce4-b9ed-353e747333f5
---

# LMDB Apple Platform Build Flags

## Apple Platform Build Configuration

On Apple targets, LMDB uses pthread process-shared mutexes instead of POSIX named semaphores, controlled by the `MDB_SKIP_POSIX_SEM` build flag. Additionally, `MDB_USE_ROBUST` is set to 0 on Apple targets because `PTHREAD_MUTEX_ROBUST` is not visible in default Apple SDK headers and is unnecessary given the flock-based instance lock. The `nostrdb-rs` `build.rs` script passes both `MDB_SKIP_POSIX_SEM` and `MDB_USE_ROBUST=0` for Apple targets, and disables stack checking for iOS targets. [^8835c-2]

## See Also

