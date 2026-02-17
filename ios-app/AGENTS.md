# iOS App

SwiftUI app with Rust FFI via UniFFI. Uses Tuist for project generation.

## ⚠️ Tuist Project - Never Edit .pbxproj

- Create Swift files in `Sources/TenexMVP/`, run `tuist generate`
- Configuration changes go in `Project.swift`

## Commands

**From `ios-app/` directory:**
```bash
tuist generate              # Generate Xcode project
tuist clean                 # Clean generated files
open TenexMVP.xcodeproj     # Open in Xcode
```

**From project root (build Rust first):**
```bash
cargo build --target aarch64-apple-ios-sim --release -p tenex-core  # Simulator
cargo build --target aarch64-apple-ios --release -p tenex-core       # Device
./scripts/generate-swift-bindings.sh                                 # Regenerate FFI
```

Generated UniFFI files are not committed. The iOS target runs a pre-build script to regenerate them automatically.

## Signing & Bundle Config

Code signing and bundle identifiers are in `Project.swift`. Don't hardcode team IDs—use Tuist's automatic signing where possible.

## Structure

```
Sources/TenexMVP/
├── Views/           # SwiftUI views
├── ViewModels/      # MVVM view models (@MainActor, @Published)
├── TenexCore/       # Swift wrappers around Rust FFI
└── TenexCoreFFI/    # UniFFI bindings (modulemap, header)
```

## Conventions

- **MVVM:** ViewModels with `@Published`, Views with `@StateObject`
- **FFI calls:** Always in `Task { }`, never synchronous on main thread
- **Errors:** Catch `TenexError` from Rust, handle by case

## Common Build Errors

- **"library not found"** → Build Rust library first
- **"module map not found"** → `tuist clean && tuist generate`
