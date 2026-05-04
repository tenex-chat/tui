import Foundation

/// Central display helper for agent/user pubkeys.
///
/// Agent names shown in UI must come from kind:0 profile metadata via
/// `TenexCoreManager.displayName(for:)`. Project status names, installed-agent
/// slugs, and config labels are not display-name sources.
enum AgentDisplayName {
    @MainActor
    static func resolve(pubkey: String, coreManager: TenexCoreManager) -> String {
        text(coreManager.displayName(for: pubkey), fallbackPubkey: pubkey)
    }

    static func text(_ kind0Name: String, fallbackPubkey pubkey: String? = nil) -> String {
        let trimmed = kind0Name.trimmingCharacters(in: .whitespacesAndNewlines)
        if !trimmed.isEmpty {
            return trimmed
        }
        guard let pubkey else { return trimmed }
        return shortPubkey(pubkey)
    }

    static func shortPubkey(_ pubkey: String) -> String {
        guard pubkey.count > 16 else { return pubkey }
        return "\(pubkey.prefix(8))...\(pubkey.suffix(8))"
    }

    @MainActor
    static func matches(pubkey: String, query: String, coreManager: TenexCoreManager) -> Bool {
        let trimmedQuery = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedQuery.isEmpty else { return true }
        let displayName = resolve(pubkey: pubkey, coreManager: coreManager)
        return displayName.localizedCaseInsensitiveContains(trimmedQuery)
            || pubkey.localizedCaseInsensitiveContains(trimmedQuery)
    }
}

/// Compatibility wrapper for older call sites. It intentionally preserves the
/// kind:0 string instead of title-casing or deriving a name from a slug.
enum AgentNameFormatter {
    static func format(_ name: String) -> String {
        AgentDisplayName.text(name)
    }
}
