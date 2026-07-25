//
//  Store.swift
//  The app's only mutable state. One `vault.list` fills the main screen —
//  ARCHITECTURE.md §7 keeps it that way, because a list that fans out into one
//  request per row is how a menu-bar app starts feeling slow.
//

import SwiftUI

@Observable
@MainActor
final class Store {
    enum Phase: Equatable {
        case idle
        case loading
        case ready
        case failed(String)
    }

    private(set) var secrets: [SecretSummary] = []
    private(set) var phase: Phase = .idle
    private(set) var uses: [UseRecord] = []

    var selected: SecretSummary?

    init() {}

    /// Preloaded, for off-screen snapshots and previews. Never used at runtime.
    init(secrets: [SecretSummary], uses: [UseRecord]) {
        self.secrets = secrets
        self.uses = uses
        self.activity = Self.bucket(uses)
        self.phase = .ready
        self.isFixture = true
    }

    /// Whether this store holds canned data. A fixture store ignores `load()`
    /// entirely — otherwise `ContentView`'s `.task` reaches for the daemon during
    /// snapshot rendering and overwrites the fixtures with whatever is live, which
    /// is precisely the non-determinism the snapshots exist to avoid.
    private var isFixture = false

    /// The bundled daemon could not be started at all — a different failure from
    /// "the socket is not answering yet", and the only one the user can act on.
    func reportDaemonUnavailable() {
        phase = .failed(Loc.t("error.daemonUnavailable"))
    }

    func load() async {
        if isFixture { return }
        if case .ready = phase {} else { phase = .loading }
        do {
            secrets = try await DaemonClient.shared.list()
            phase = .ready
        } catch {
            // Keep whatever was on screen. A transient socket drop should not blank
            // a list the user is reading — it should say so and leave it visible.
            phase = .failed(error.localizedDescription)
        }
    }

    /// Uses per day for the last fourteen days, oldest first — the detail view's
    /// activity strip. Derived from `uses` rather than requested separately: one
    /// round trip already carries the data.
    private(set) var activity: [Int] = Array(repeating: 0, count: 14)

    func select(_ secret: SecretSummary) async {
        selected = secret
        uses = []
        activity = Array(repeating: 0, count: 14)
        uses = (try? await DaemonClient.shared.uses(of: secret.name)) ?? []
        activity = Self.bucket(uses)
    }

    static func bucket(_ uses: [UseRecord]) -> [Int] {
        var days = Array(repeating: 0, count: 14)
        let calendar = Calendar.current
        let today = calendar.startOfDay(for: .now)
        for use in uses {
            let day = calendar.startOfDay(for: use.at)
            guard let delta = calendar.dateComponents([.day], from: day, to: today).day,
                  (0..<14).contains(delta)
            else { continue }
            // Oldest on the left, so index 13 is today.
            days[13 - delta] += 1
        }
        return days
    }

    var liveCount: Int { secrets.filter { $0.live != nil }.count }

    func stopSession(_ token: String) async {
        try? await DaemonClient.shared.closeSession(token)
        await load()
    }

    /// Refresh while anything is being forwarded, so request counts move.
    ///
    /// Polls only while a session is open and only while the view is on screen —
    /// SwiftUI cancels the task on disappear. A menu-bar app that polls a socket
    /// forever to render a number nobody is looking at is a battery bug.
    func followLiveSessions() async {
        while !Task.isCancelled {
            try? await Task.sleep(for: .seconds(liveCount > 0 ? 2 : 6))
            if Task.isCancelled { return }
            await load()
        }
    }

    func rotate(_ secret: SecretSummary, value: String) async -> String? {
        do {
            try await DaemonClient.shared.rotate(name: secret.name, value: value)
            await load()
            await reselect(secret.name)
            return nil
        } catch {
            return error.localizedDescription
        }
    }

    func rename(_ secret: SecretSummary, to display: String) async -> String? {
        do {
            try await DaemonClient.shared.setDisplay(name: secret.name, display: display)
            await load()
            await reselect(secret.name)
            return nil
        } catch {
            return error.localizedDescription
        }
    }

    func remove(_ secret: SecretSummary) async -> String? {
        do {
            try await DaemonClient.shared.remove(name: secret.name)
            selected = nil
            uses = []
            await load()
            return nil
        } catch {
            return error.localizedDescription
        }
    }

    /// Re-point `selected` at the freshly loaded row, so the detail pane shows the
    /// new mask or label instead of the stale copy it was holding.
    private func reselect(_ name: String) async {
        guard let updated = secrets.first(where: { $0.name == name }) else {
            selected = nil
            return
        }
        selected = updated
    }

    func add(display: String, value: String, upstream: String?) async -> String? {
        guard let name = Slug.make(from: display) else {
            return String(localized: "add.error.noSlug")
        }
        do {
            try await DaemonClient.shared.add(
                name: name, display: display, value: value, upstream: upstream
            )
            await load()
            return nil
        } catch {
            return error.localizedDescription
        }
    }
}

extension Store {
    /// Runs a whole import (ARCHITECTURE.md §10.5) and returns what it did, so the
    /// same screen can offer to undo it.
    ///
    /// Ordering is the only interesting part and it is not negotiable: back up
    /// every file first, then store every value, and only then rewrite anything.
    /// A rewrite that lands before its value is in the keychain leaves a project
    /// whose `.env` points at a reference that resolves to nothing — the file no
    /// longer holds the credential and neither does Keyward.
    func runImport(
        _ scan: ImportScan,
        addingToGitignore ignorePaths: [String]
    ) async -> Result<ImportReceipt, Error> {
        var receipt = ImportReceipt(folder: scan.folder, backups: ImportWriter.backupDirectory())
        let touched = scan.files.filter { !$0.included.isEmpty }

        do {
            var toBackUp = touched.map(\.url)
            let fm = FileManager.default
            for extra in [scan.agent.url, scan.gitignore]
            where fm.fileExists(atPath: extra.path(percentEncoded: false)) {
                toBackUp.append(extra)
            }
            try ImportWriter.backUp(toBackUp, into: &receipt)

            // One `vault.add` per distinct value. `assignSlugs` already collapsed
            // duplicates onto one slug, so a value stored twice would be the same
            // slug twice and the second call would fail on a name that exists.
            var stored = Set<String>()
            var listing: [(ref: String, display: String)] = []
            for file in touched {
                for finding in file.included where !stored.contains(finding.slug) {
                    try await DaemonClient.shared.add(
                        name: finding.slug,
                        display: finding.display,
                        value: finding.value,
                        upstream: nil
                    )
                    stored.insert(finding.slug)
                    receipt.addedSecrets.append(finding.slug)
                    listing.append((finding.ref, finding.display))
                }
            }

            for file in touched {
                try ImportWriter.write(file.rewritten(), to: file.url)
                receipt.rewrittenCount += 1
            }

            try ImportWriter.writeAgentBlock(
                AgentBlock.text(listing: listing), to: scan.agent, receipt: &receipt
            )
            try ImportWriter.addToGitignore(
                ignorePaths, at: scan.gitignore, receipt: &receipt
            )

            await load()
            return .success(receipt)
        } catch {
            // Whatever got as far as being written is rolled back here rather than
            // left for the user to notice. A failed import that half-rewrote a
            // `.env` is worse than one that did nothing.
            _ = ImportWriter.undo(receipt)
            for name in receipt.addedSecrets {
                try? await DaemonClient.shared.remove(name: name)
            }
            await load()
            return .failure(error)
        }
    }

    /// The one-click undo §10.5 requires. Returns the files it could not restore.
    func undoImport(_ receipt: ImportReceipt) async -> [String] {
        let problems = ImportWriter.undo(receipt)
        for name in receipt.addedSecrets {
            try? await DaemonClient.shared.remove(name: name)
        }
        await load()
        return problems
    }
}

/// Mirrors `Reference::slugify` in `keyward-core`. Kept deliberately identical,
/// including the refusal: a display name with no ASCII has no predictable slug,
/// and transliterating "生产数据库" would hand the user a reference they cannot
/// recognise in their own `.env`.
enum Slug {
    static func make(from display: String) -> String? {
        var out = ""
        var previousDash = false
        for ch in display {
            if ch.isASCII, ch.isLetter || ch.isNumber {
                out.append(Character(ch.lowercased()))
                previousDash = false
            } else if !out.isEmpty, !previousDash {
                out.append("-")
                previousDash = true
            }
        }
        while out.hasSuffix("-") { out.removeLast() }
        if out.count > 64 { out = String(out.prefix(64)) }
        while out.hasSuffix("-") { out.removeLast() }
        return out.isEmpty ? nil : out
    }
}

extension Date {
    /// "2 分钟前" / "2 min ago". `RelativeDateTimeFormatter` localises for free,
    /// which is most of what the bilingual requirement needs from this layer.
    var relative: String {
        let f = RelativeDateTimeFormatter()
        f.unitsStyle = .short
        f.locale = Loc.locale
        return f.localizedString(for: self, relativeTo: .now)
    }
}
