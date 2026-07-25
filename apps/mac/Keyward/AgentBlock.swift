//
//  AgentBlock.swift
//  The `CLAUDE.md` / `AGENTS.md` section the import flow writes.
//
//  This used to also back a "For your AI" button in the toolbar, which handed the
//  user a paragraph to paste by hand. That button existed only because nothing
//  connected `kw mcp` to anything: with the MCP server registered, the agent asks
//  Keyward for a reference itself and needs no prose. The button is gone; the
//  block survives because the import flow still writes it for agents that are not
//  connected.
//

import Foundation

enum AgentBlock {
    /// Takes pairs rather than `SecretSummary` so the import can list references
    /// it is about to create — they are not in the vault yet, and a block that
    /// omits them tells the agent the project's own keys do not exist.
    static func text(listing references: [(ref: String, display: String)]) -> String {
        let lines = references
            .map { "- `\($0.ref)` — \($0.display)" }
            .joined(separator: "\n")

        return """
        \(Loc.t("agent.heading"))

        \(Loc.t("agent.intro"))

        \(Loc.t("agent.rule1"))
        \(Loc.t("agent.rule2"))
        \(Loc.t("agent.rule3"))
        \(Loc.t("agent.rule4"))

        \(Loc.t("agent.available"))

        \(lines.isEmpty ? Loc.t("agent.noSecrets") : lines)
        """
    }
}
