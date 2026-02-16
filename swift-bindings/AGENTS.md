# Swift Bindings

**⚠️ AUTO-GENERATED - DO NOT EDIT MANUALLY**

Contains UniFFI-generated Swift bindings from Rust. Files are overwritten on regeneration.

## Contents

- `tenex_core.swift` - Swift interface to Rust
- `tenex_coreFFI.h` - C header
- `tenex_coreFFI.modulemap` - Swift module map

## Regenerate

```bash
./scripts/generate-swift-bindings.sh
```

**When to regenerate:**
- After changing `#[uniffi::export]` functions in `crates/tenex-core/src/ffi.rs`
- After adding/removing UniFFI types or enums
- When Swift compilation errors suggest FFI mismatch
