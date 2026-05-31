---
title: LMDB Configuration and Mapsize
slug: lmdb-configuration-and-mapsize
summary: The LMDB mapsize is set to 16GB (up from 8GB) to prevent the database from hitting the mapsize ceiling
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
  - session:4a3a2b00-76bb-4082-b058-d72541437c52
---

# LMDB Configuration and Mapsize

## LMDB Configuration and MapSize

The LMDB map size is configured to 16 GiB (up from 8 GiB) to prevent the database from hitting the mapsize ceiling. This reserves virtual address space without allocating physical RAM; the OS page cache determines physical RAM usage based on the actual working set accessed, such as recent events and a few profiles. MDB_NOLOCK is not used because it disables LMDB's reader table, causing the writer to reuse pages that active reader threads still hold, resulting in memory corruption. The TUI accesses LMDB via memory-mapped files rather than loading the entire database into memory.

<!-- citations: [^8835c-3] [^4a3a2-1] -->
## See Also

