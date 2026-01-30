import Foundation

/// Unified error type for safe FFI operations.
/// Wraps TenexError and adds additional failure cases.
enum CoreError: LocalizedError {
    /// Error from the underlying TenexCore FFI
    case tenex(TenexError)
    /// Core not initialized
    case notInitialized

    var errorDescription: String? {
        switch self {
        case .tenex(let error):
            return error.localizedDescription
        case .notInitialized:
            return "Core not initialized"
        }
    }
}
