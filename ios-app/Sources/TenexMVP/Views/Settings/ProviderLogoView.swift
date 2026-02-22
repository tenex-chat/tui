import SwiftUI

private let providerLogoSlugs: [String: String] = [
    "openrouter": "openrouter",
    "elevenlabs": "elevenlabs",
    "anthropic": "anthropic",
    "openai": "openai",
    "ollama": "ollama",
]

struct ProviderLogoView: View {
    let provider: String
    let size: CGFloat

    #if os(macOS)
    @State private var image: NSImage?
    #endif

    init(_ provider: String, size: CGFloat = 24) {
        self.provider = provider
        self.size = size
    }

    var body: some View {
        Group {
            #if os(macOS)
            if let image {
                Image(nsImage: image)
                    .resizable()
                    .aspectRatio(contentMode: .fit)
            } else {
                Color.clear
            }
            #else
            RoundedRectangle(cornerRadius: 4)
                .fill(Color.secondary.opacity(0.3))
            #endif
        }
        .frame(width: size, height: size)
        #if os(macOS)
        .task(id: provider) {
            self.image = await ProviderLogoCache.shared.logo(for: provider)
        }
        #endif
    }
}

#if os(macOS)
actor ProviderLogoCache {
    static let shared = ProviderLogoCache()
    private var cache: [String: NSImage] = [:]

    func logo(for provider: String) async -> NSImage? {
        let slug = providerLogoSlugs[provider] ?? provider
        if let cached = cache[slug] { return cached }
        guard let url = URL(string: "https://models.dev/logos/\(slug).svg") else { return nil }
        do {
            let (data, _) = try await URLSession.shared.data(from: url)
            guard let image = NSImage(data: data) else { return nil }
            image.isTemplate = true
            cache[slug] = image
            return image
        } catch {
            return nil
        }
    }
}
#endif
