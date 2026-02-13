# iOS App - Agent Guidelines

## ⚠️ CRITICAL: Tuist Project - DO NOT Edit .pbxproj Directly

**This is a Tuist-managed project.** The `TenexMVP.xcodeproj/project.pbxproj` file is **auto-generated** and gitignored.

### Adding New Swift Files
When adding new Swift files to the project:
1. **Just create the file** in the correct directory under `Sources/TenexMVP/`
2. **Run `tuist generate`** to regenerate the Xcode project
3. The glob pattern `Sources/TenexMVP/**/*.swift` in `Project.swift` will automatically include all Swift files

**NEVER:**
- Manually edit `project.pbxproj` - changes will be lost on regeneration
- Try to add file references directly to Xcode

**ALWAYS:**
- Create Swift files in the proper directory structure
- Run `tuist generate` after adding/removing files
- Edit `Project.swift` for configuration changes (dependencies, settings, etc.)

---

## Overview

Native iOS app built with:
- **SwiftUI** - Modern declarative UI framework
- **Tuist** - Xcode project generator for clean project management
- **Rust FFI** - Core logic via libtenex_core.a static library
- **MVVM Architecture** - ViewModels for business logic, Views for UI

**Target iOS:** 26.0+
**Bundle ID:** com.tenex.mvp
**Code Signing:** SANITY ISLAND LLC (Team: 456SHKPP26)

## Architecture

```
┌─────────────────────────────────────────────┐
│            Views/ (SwiftUI)                 │
│  - ConversationsView                        │
│  - InboxView, LoginView, etc.               │
└─────────────────┬───────────────────────────┘
                  │
┌─────────────────▼───────────────────────────┐
│         ViewModels/ (MVVM)                  │
│  - ConversationsViewModel                   │
│  - Business logic, state management         │
└─────────────────┬───────────────────────────┘
                  │
┌─────────────────▼───────────────────────────┐
│      TenexCore/ (Swift Wrappers)           │
│  - Swift-friendly API over FFI              │
│  - Task-based async/await                   │
└─────────────────┬───────────────────────────┘
                  │
┌─────────────────▼───────────────────────────┐
│   TenexCoreFFI/ (UniFFI Bindings)          │
│  - Auto-generated Swift code               │
│  - FFI headers and modulemap                │
│  - Links to libtenex_core.a                 │
└─────────────────────────────────────────────┘
```

## Project Structure

```
ios-app/
├── Project.swift              # Tuist project definition
├── Sources/TenexMVP/
│   ├── App.swift             # Main app entry point
│   ├── ContentView.swift     # Root view
│   ├── Views/                # SwiftUI views
│   ├── ViewModels/           # MVVM view models
│   ├── Models/               # Swift data models
│   ├── Services/             # Service layer (networking, etc.)
│   ├── TenexCore/            # Swift wrappers around Rust
│   ├── TenexCoreFFI/         # FFI bindings (modulemap, header)
│   ├── Resources/            # Assets, localization
│   └── Profiling/            # Performance analysis tools
├── build/                     # Xcode build artifacts
└── TenexMVP.xcodeproj/       # Generated Xcode project
```

## Commands

### Project Generation
```bash
# Generate Xcode project (run from ios-app/)
cd ios-app
tuist generate

# Clean Tuist cache
tuist clean

# Open in Xcode
open TenexMVP.xcodeproj
```

### Building Rust Dependencies
**Before running iOS app, build Rust library:**

```bash
# For simulator (from project root)
cargo build --target aarch64-apple-ios-sim --release -p tenex-core

# For device
cargo build --target aarch64-apple-ios --release -p tenex-core

# Regenerate Swift bindings after Rust changes
./scripts/generate-swift-bindings.sh
```

### Running
1. Build Rust library (see above)
2. Open Xcode project: `open ios-app/TenexMVP.xcodeproj`
3. Select target device/simulator
4. Run (⌘R)

## FFI Integration

### Library Paths
Configured in Project.swift:

**Simulator:**
- Library: `../target/aarch64-apple-ios-sim/release/libtenex_core.a`
- Search path: `LIBRARY_SEARCH_PATHS[sdk=iphonesimulator*]`

**Device:**
- Library: `../target/aarch64-apple-ios/release/libtenex_core.a`
- Search path: `LIBRARY_SEARCH_PATHS[sdk=iphoneos*]`

### Modulemap & Headers
Located in `Sources/TenexMVP/TenexCoreFFI/`:
- `tenex_coreFFI.h` - C header with FFI declarations
- `tenex_coreFFI.modulemap` - Swift module map
- Generated Swift bindings live in `swift-bindings/tenex_core.swift` and are included directly by the iOS target

### Swift Import Paths
```swift
// Import the FFI module
import tenex_coreFFI

// Or use via wrappers in TenexCore/
import TenexMVP
```

## Conventions

### Code Style
- **SwiftUI Views:** Declarative, composable, minimal state
- **ViewModels:** Observable objects with `@Published` properties
- **Async/Await:** Use Task for calling Rust FFI functions
- **Naming:** camelCase for variables/functions, PascalCase for types

### MVVM Pattern
```swift
// ViewModel
@MainActor
class MyViewModel: ObservableObject {
    @Published var data: [Item] = []

    func fetchData() async {
        // Call Rust FFI via wrapper
        do {
            data = try await TenexCoreWrapper.fetchItems()
        } catch {
            print("Error: \(error)")
        }
    }
}

// View
struct MyView: View {
    @StateObject var viewModel = MyViewModel()

    var body: some View {
        List(viewModel.data) { item in
            Text(item.name)
        }
        .task {
            await viewModel.fetchData()
        }
    }
}
```

### FFI Calls
Always wrap FFI calls in async context:

```swift
// ✅ Good
Task {
    do {
        let result = try await tenexCore.fetchProjects()
        await MainActor.run {
            self.projects = result
        }
    } catch {
        print("Error: \(error)")
    }
}

// ❌ Bad - synchronous FFI call blocks UI
let result = tenexCore.fetchProjectsSync() // Don't do this
```

### Error Handling
```swift
do {
    let data = try tenexCore.someOperation()
    // Success path
} catch let error as TenexError {
    // Handle specific Rust errors
    switch error {
    case .database(let msg):
        print("DB Error: \(msg)")
    case .network(let msg):
        print("Network Error: \(msg)")
    }
} catch {
    // Handle other errors
    print("Unexpected error: \(error)")
}
```

## Common Patterns

### Calling Rust from Swift

**Wrapper Pattern (Recommended):**
```swift
// In TenexCore/TenexCoreWrapper.swift
class TenexCoreWrapper {
    static func fetchProjects() async throws -> [Project] {
        try await withCheckedThrowingContinuation { continuation in
            Task.detached {
                do {
                    let core = try TenexCore(dbPath: "...")
                    let projects = try core.getProjects()
                    continuation.resume(returning: projects)
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }
}
```

### State Management

**Use @Published for reactive updates:**
```swift
@MainActor
class ConversationsViewModel: ObservableObject {
    @Published var conversations: [Conversation] = []
    @Published var isLoading = false
    @Published var error: String?

    func refresh() async {
        isLoading = true
        defer { isLoading = false }

        do {
            conversations = try await fetchConversations()
            error = nil
        } catch {
            error = error.localizedDescription
        }
    }
}
```

### Image Loading
Uses Kingfisher dependency:

```swift
import Kingfisher

KFImage(URL(string: avatarUrl))
    .placeholder {
        ProgressView()
    }
    .resizable()
    .frame(width: 40, height: 40)
    .clipShape(Circle())
```

## Debugging

### Xcode Console
View Rust logs and Swift logs:
- Set breakpoints in Swift code
- Rust panics appear in console
- Use `print()` for quick debugging

### Instruments
- Profile with Xcode Instruments (⌘I)
- Time Profiler for performance
- Allocations for memory leaks

### Common Issues

**Build Errors:**
1. **"library not found for -ltenex_core"**
   - Build Rust library first: `cargo build --target aarch64-apple-ios-sim --release -p tenex-core`

2. **"module map not found"**
   - Regenerate Tuist project: `tuist clean && tuist generate`
   - Check Swift include paths contain `TenexCoreFFI/`

3. **Framework autolink errors**
   - Already handled in Project.swift with `-disable-autolink-framework` flags

**Runtime Errors:**
1. **Rust panic**
   - Check console for panic message
   - Enable Rust logging: Set environment variable `RUST_LOG=debug`

2. **Thread errors**
   - Ensure UI updates happen on MainActor
   - Wrap FFI calls in Task.detached if needed

## Testing

### Manual Testing
- Use ios-tester agent for workflow documentation
- Test on both simulator and device
- Verify different iOS versions

### Unit Tests
```swift
import XCTest
@testable import TenexMVP

class MyTests: XCTestCase {
    func testSomething() async throws {
        let viewModel = MyViewModel()
        await viewModel.fetchData()
        XCTAssertFalse(viewModel.data.isEmpty)
    }
}
```

## Performance

### Profiling Tools
Located in `Sources/TenexMVP/Profiling/`:
- Activity tracking
- Performance measurement

See `PERFORMANCE_ANALYSIS.md` in ios-app/ for detailed analysis.

### Best Practices
- Minimize FFI calls (batch when possible)
- Use lazy loading for large lists
- Cache expensive computations
- Profile regularly with Instruments

## Permissions

Defined in Project.swift:
- **Microphone:** `NSMicrophoneUsageDescription` - Voice dictation
- **Speech Recognition:** `NSSpeechRecognitionUsageDescription` - Voice-to-text

## Related

- [../AGENTS.md](../AGENTS.md) - Project overview
- [../crates/tenex-core/AGENTS.md](../crates/tenex-core/AGENTS.md) - Rust core details
- [PERFORMANCE_ANALYSIS.md](./PERFORMANCE_ANALYSIS.md) - Performance profiling
- [Project.swift](./Project.swift) - Tuist configuration
