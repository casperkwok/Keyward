//
//  UseWithAISheet.swift
//  One sentence to copy, and nothing else.
//
//  Three earlier versions each asked the user to do the wiring themselves. The
//  first pasted prose into `CLAUDE.md`. The second detected the installed agents
//  and edited their config files for them — a `~/.claude.json` with a hundred
//  projects in it, plus backups and a TOML appender, all to save one copy-paste.
//  The third handed over a block of JSON and a list of file paths: "Claude Code:
//  ~/.claude.json · Codex: ~/.codex/config.toml…".
//
//  All three shared a flaw. The person this is built for does not open config
//  files — that is most of why they have a coding agent at all. Handing them
//  JSON and a path is handing them the job.
//
//  The agent, on the other hand, edits config files for a living. So this screen
//  is one sentence addressed to it, and the user's entire task is to paste that
//  sentence into the conversation they already have open.
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

    private var message: String {
        Loc.t("use.message %@", MCPServer.url)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            VStack(alignment: .leading, spacing: 7) {
                Text("use.title")
                    .font(.system(size: 19, weight: .semibold))
                    .tracking(Kind.headingTracking)
                Text("use.subtitle")
                    .font(Kind.meta)
                    .foregroundStyle(Tint.ink2)
                    .fixedSize(horizontal: false, vertical: true)
            }

            Text(verbatim: message)
                .font(.system(size: 14))
                .foregroundStyle(Tint.ink)
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(15)
                .background(Tint.surface3, in: RoundedRectangle(cornerRadius: Radius.card))
                .overlay(
                    RoundedRectangle(cornerRadius: Radius.card)
                        .strokeBorder(Tint.line, lineWidth: 0.5)
                )

            // Shown only when it is wrong. A green light that is green every time
            // teaches nobody anything; the one state worth a line here is the one
            // where sending the message would not work.
            if !endpointUp {
                Text("mcp.down")
                    .font(Kind.meta)
                    .foregroundStyle(Tint.amber)
                    .fixedSize(horizontal: false, vertical: true)
            }

            // Without this the user has copied a sentence and no idea what it
            // buys them.
            Text(Loc.t("use.after %@", exampleName))
                .font(Kind.meta)
                .foregroundStyle(Tint.ink2)
                .fixedSize(horizontal: false, vertical: true)

            HStack(spacing: 8) {
                Spacer()
                Button("action.done") { dismiss() }
                    .buttonStyle(GhostButtonStyle())
                    .keyboardShortcut(.cancelAction)
                Button(copied ? "action.copied" : "use.copy") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(message, forType: .string)
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
        .frame(width: 460)
        .background(Tint.surface)
        .task {
            while !Task.isCancelled {
                endpointUp = MCPServer.isListening()
                try? await Task.sleep(for: .seconds(2))
            }
        }
    }
}
