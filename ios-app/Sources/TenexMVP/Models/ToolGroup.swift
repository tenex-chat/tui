import Foundation

/// A group of tools for display in the agent config sheet.
/// Groups tools by MCP server prefix or common underscore prefix.
struct ToolGroup: Identifiable {
    let id = UUID()
    let name: String
    let tools: [String]
    var isExpanded: Bool

    /// Check if all tools in the group are selected
    func isFullySelected(_ selectedTools: Set<String>) -> Bool {
        tools.allSatisfy { selectedTools.contains($0) }
    }

    /// Check if some (but not all) tools in the group are selected
    func isPartiallySelected(_ selectedTools: Set<String>) -> Bool {
        let count = tools.filter { selectedTools.contains($0) }.count
        return count > 0 && count < tools.count
    }

    /// Build tool groups from a list of tools.
    /// Groups by MCP server prefix (mcp__server__method) or common underscore prefix.
    static func buildGroups(from allTools: [String]) -> [ToolGroup] {
        var groups: [String: [String]] = [:]
        var ungrouped: [String] = []

        for tool in allTools {
            // MCP tools: mcp__<server>__<method>
            if tool.hasPrefix("mcp__") {
                let mcpParts = tool.split(separator: "__")
                if mcpParts.count >= 2 {
                    let groupKey = "MCP: \(mcpParts[1])"
                    groups[groupKey, default: []].append(tool)
                    continue
                }
            }

            // Find common prefixes (underscore-separated)
            if let prefixEnd = tool.firstIndex(of: "_") {
                let prefix = String(tool[..<prefixEnd])
                // Only group if there are at least 2 tools with this prefix
                let similarCount = allTools.filter { $0.hasPrefix("\(prefix)_") }.count
                if similarCount >= 2 {
                    let groupKey = prefix.uppercased()
                    groups[groupKey, default: []].append(tool)
                    continue
                }
            }

            // No group found - add to ungrouped
            ungrouped.append(tool)
        }

        var result: [ToolGroup] = []

        // Add grouped tools (sorted by group name)
        for groupName in groups.keys.sorted() {
            var tools = groups[groupName] ?? []
            tools.sort()
            // Remove duplicates
            tools = Array(Set(tools)).sorted()
            result.append(ToolGroup(
                name: groupName,
                tools: tools,
                isExpanded: false
            ))
        }

        // Add ungrouped tools as single-item groups
        for tool in ungrouped.sorted() {
            result.append(ToolGroup(
                name: tool,
                tools: [tool],
                isExpanded: true  // Single tools are always "expanded"
            ))
        }

        return result
    }
}
