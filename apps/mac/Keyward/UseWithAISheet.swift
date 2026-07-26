//
//  UseWithAISheet.swift
//  How to get a coding agent using Keyward: one snippet, one sentence.
//
//  Two earlier versions were heavier and both were wrong in the same direction.
//  The first handed the user a paragraph of prose to paste into `CLAUDE.md`,
//  which only existed because nothing connected `kw mcp` to anything. The second
//  detected the installed agents and wrote their config files itself — which
//  meant editing a `~/.claude.json` with a hundred projects in it, plus backups,
//  a TOML appender and a format-detection path, all to save one copy-paste.
//
//  An MCP config block is universal: every agent takes the same three keys, the
//  user pastes it where they already keep such things, and Keyward never touches
//  a file it does not own.
//

import SwiftUI

struct UseWithAISheet: View {
    @Environment(Store.self) private var store
    @Environment(\.dismiss) private var dismiss
    @State private var copied = false
    @State private var endpointUp = true

    /// A real name from the vault, because "use the stripe key" only teaches
    /// anything if `stripe` is a key you have.
    private var exampleName: String {
        store.secrets.first?.name ?? "stripe"
    }

    /// A URL, not a path into the app bundle.
    ///
    /// This used to name `kw` by absolute path, which is what the stdio transport
    /// requires. Two things were wrong with it: it asked a person who has never
    /// opened a terminal to reason about where files live, and it went stale the
    /// moment the app was moved. The endpoint answers at the same address for
    /// ever.
    private var snippet: String {
        """
        {
          "mcpServers": {
            "keyward": { "type": "http", "url": "\(MCPServer.url)" }
          }
        }
        """
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            VStack(alignment: .leading, spacing: 6) {
                Text("use.title")
                    .font(.system(size: 19, weight: .semibold))
                    .tracking(Kind.headingTracking)
                Text("use.subtitle")
                    .font(Kind.meta)
                    .foregroundStyle(Tint.ink2)
                    .fixedSize(horizontal: false, vertical: true)
            }

            VStack(alignment: .leading, spacing: 8) {
                Text("use.step1")
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(Tint.ink)
                Text(snippet)
                    .font(.system(size: 11.5, design: .monospaced))
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(13)
                    .background(Tint.surface3, in: RoundedRectangle(cornerRadius: Radius.card))
                    .overlay(
                        RoundedRectangle(cornerRadius: Radius.card)
                            .strokeBorder(Tint.line, lineWidth: 0.5)
                    )
                // `verbatim`, because `Text` parses Markdown and this line is
                // full of `~/` paths — a pair of tildes is strikethrough, so the
                // config locations rendered with a line through them.
                // Shown only when it is wrong. A green dot that is green every
                // time teaches nobody anything; the one case worth a line on this
                // screen is the one where pasting the config would not work.
                if !endpointUp {
                    Text("mcp.down")
                        .font(Kind.meta)
                        .foregroundStyle(Tint.amber)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Text(verbatim: Loc.t("use.where"))
                    .font(Kind.meta)
                    .foregroundStyle(Tint.ink3)
                    .fixedSize(horizontal: false, vertical: true)
            }

            VStack(alignment: .leading, spacing: 8) {
                Text("use.step2")
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(Tint.ink)
                Text(Loc.t("use.example %@", exampleName))
                    .font(.system(size: 14))
                    .foregroundStyle(Tint.ink)
                    .padding(.horizontal, 13)
                    .padding(.vertical, 11)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(Tint.surface3, in: RoundedRectangle(cornerRadius: Radius.card))
                // Without this line a reader assumes "use the key" means the
                // agent is handed one, which is the opposite of the product.
                Text(Loc.t("use.whatHappens %@", exampleName))
                    .font(Kind.meta)
                    .foregroundStyle(Tint.ink2)
                    .fixedSize(horizontal: false, vertical: true)
            }

            HStack(spacing: 8) {
                Spacer()
                Button("action.done") { dismiss() }
                    .buttonStyle(GhostButtonStyle())
                    .keyboardShortcut(.cancelAction)
                Button(copied ? "action.copied" : "use.copy") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(snippet, forType: .string)
                    copied = true
                    Task {
                        try? await Task.sleep(for: .seconds(1.5))
                        copied = false
                    }
                }
                .buttonStyle(PrimaryButtonStyle())
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding(24)
        .frame(width: 480)
        .background(Tint.surface)
        .task {
            while !Task.isCancelled {
                endpointUp = MCPServer.isListening()
                try? await Task.sleep(for: .seconds(2))
            }
        }
    }
}
