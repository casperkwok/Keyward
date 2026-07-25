//
//  ApprovalPrompt.swift
//  The prompt that answers `Approval::Ask` (ARCHITECTURE.md §4).
//
//  The daemon parks a `secret.hand` call for sixty seconds waiting for a human.
//  Until this existed nothing in the GUI answered, so a secret set to "ask" was a
//  secret that could not be used: every request timed out. The CLI grew a
//  terminal fallback, but a setting whose only interface is a terminal is not a
//  setting a consumer can rely on.
//

import AppKit
import SwiftUI

/// One request waiting on a human.
struct PendingApproval: Codable, Hashable, Identifiable {
    let id: String
    let secret: String
    /// What asked, as the caller reported it.
    let actor: String?
    /// What the kernel says the caller is: the executable's path, resolved from
    /// the connection rather than taken from the request. This is the line that
    /// decides the answer.
    let caller: String?
    let purpose: String?
    let project: String?
    let expiresIn: Int

    enum CodingKeys: String, CodingKey {
        case id, secret, actor, caller, purpose, project
        case expiresIn = "expires_in"
    }
}

/// Polls for pending approvals and shows a window for the first one.
///
/// A window rather than a sheet: the request arrives while the user is in a
/// terminal, not in Keyward, and a sheet attached to a window they cannot see is
/// a prompt that times out unanswered.
@MainActor
@Observable
final class ApprovalWatcher {
    private(set) var pending: PendingApproval?
    private var window: NSWindow?
    private var task: Task<Void, Never>?

    func start() {
        guard task == nil else { return }
        task = Task { [weak self] in
            while !Task.isCancelled {
                await self?.poll()
                // 700ms: fast enough that the prompt appears while the user is
                // still looking at the terminal that triggered it, slow enough
                // that an idle machine is not waking a socket sixty times a
                // minute for nothing.
                try? await Task.sleep(for: .milliseconds(700))
            }
        }
    }

    func stop() {
        task?.cancel()
        task = nil
    }

    private func poll() async {
        let queue = (try? await DaemonClient.shared.pendingApprovals()) ?? []
        // Only ever one on screen. Two prompts stacked over each other is how a
        // user ends up allowing the one they meant to deny.
        guard let next = queue.first else {
            if pending != nil { dismiss() }
            return
        }
        if pending?.id == next.id {
            pending = next
            return
        }
        pending = next
        present(next)
    }

    private func present(_ request: PendingApproval) {
        closeWindow()
        pending = request
        let controller = NSHostingController(
            rootView: ApprovalPromptView(watcher: self)
                .environment(self)
                .tint(Tint.accent)
        )
        let window = NSWindow(contentViewController: controller)
        window.title = ""
        window.styleMask = [.titled, .fullSizeContentView]
        window.titlebarAppearsTransparent = true
        window.titleVisibility = .hidden
        window.isMovableByWindowBackground = true
        // Above whatever the user is actually looking at, which is the terminal
        // that is currently blocked waiting for this answer.
        window.level = .floating
        window.center()
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        self.window = window
    }

    private func closeWindow() {
        window?.close()
        window = nil
    }

    private func dismiss() {
        closeWindow()
        pending = nil
    }

    func answer(_ decision: String) {
        guard let id = pending?.id else { return }
        dismiss()
        Task { try? await DaemonClient.shared.resolveApproval(id: id, decision: decision) }
    }
}

struct ApprovalPromptView: View {
    let watcher: ApprovalWatcher
    @Environment(ApprovalWatcher.self) private var live

    var body: some View {
        let request = live.pending
        VStack(alignment: .leading, spacing: 18) {
            HStack(alignment: .top, spacing: 14) {
                Image(systemName: "hand.raised")
                    .font(.system(size: 17, weight: .medium))
                    .foregroundStyle(Tint.amber)
                    .frame(width: 34, height: 34)
                    .background(Tint.amberWash, in: RoundedRectangle(cornerRadius: Radius.card))

                VStack(alignment: .leading, spacing: 5) {
                    Text("approve.title \(request?.secret ?? "")")
                        .font(.system(size: 17, weight: .semibold))
                        .tracking(Kind.headingTracking)
                    // The consequence, not the mechanism. "Injection tier" is
                    // true and useless; this is what actually happens.
                    Text("approve.consequence")
                        .font(Kind.meta)
                        .foregroundStyle(Tint.ink2)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }

            VStack(alignment: .leading, spacing: 7) {
                detail("approve.who", request?.purpose ?? request?.actor ?? "—")
                if let project = request?.project {
                    detail("approve.project", project)
                }
                // Shown last and in full: an unverified path is the whole reason
                // to hesitate, and truncating it would hide the part that differs
                // between the tool you meant and the one you did not.
                detail("approve.caller", request?.caller ?? Loc.t("approve.unknownCaller"))
            }
            .padding(14)
            .background(Tint.surface3, in: RoundedRectangle(cornerRadius: Radius.card))
            .overlay(
                RoundedRectangle(cornerRadius: Radius.card)
                    .strokeBorder(Tint.line, lineWidth: 0.5)
            )

            HStack(spacing: 8) {
                Text("approve.countdown \(request?.expiresIn ?? 0)")
                    .font(Kind.meta)
                    .foregroundStyle(Tint.ink3)
                    .monospacedDigit()
                    .fixedSize()
                Spacer(minLength: Space.m)
                // Deny is the default action, so Return and Escape both refuse.
                // A prompt where the fast path discloses a secret is a prompt
                // that trains people to disclose secrets.
                Button("approve.deny") { watcher.answer("deny") }
                    .buttonStyle(PrimaryButtonStyle())
                    .keyboardShortcut(.defaultAction)
                    .fixedSize()
                Button("approve.once") { watcher.answer("allow_once") }
                    .buttonStyle(GhostButtonStyle())
                    .fixedSize()
                Button("approve.always") { watcher.answer("allow_caller") }
                    .buttonStyle(GhostButtonStyle())
                    .fixedSize()
            }
        }
        .padding(24)
        .frame(width: 520)
        .background(Tint.surface)
    }

    private func detail(_ label: LocalizedStringKey, _ value: String) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Text(label)
                .font(.system(size: 11.5, weight: .medium))
                .foregroundStyle(Tint.ink3)
                .frame(width: 68, alignment: .leading)
            Text(value)
                .font(.system(size: 11.5, design: .monospaced))
                .foregroundStyle(Tint.ink)
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}
