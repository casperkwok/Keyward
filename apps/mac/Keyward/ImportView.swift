//
//  ImportView.swift
//  The import screen (ARCHITECTURE.md §10.5) — a folder in, a before/after diff
//  out, one button.
//
//  It is the only screen in the product that writes to files the user owns, so it
//  shows the whole change before making any of it and keeps the undo visible after.
//  The alternative — a progress bar and a summary — asks the user to trust a
//  rewrite of their source files on the strength of a sentence.
//

import AppKit
import SwiftUI

struct ImportView: View {
    @Environment(Store.self) private var store
    @Environment(\.dismiss) private var dismiss

    /// A folder dropped on the window, so the drop and the menu entry land on the
    /// same screen rather than two that have to be kept in step.
    var folder: URL?
    /// Preloaded, for snapshots. A scan of a real folder is by definition not
    /// reproducible, and this screen is the one most worth reviewing as an image.
    var preview: ImportScan?

    private enum Phase: Equatable {
        case choosing
        case review
        case applying
        case done
        case failed(String)
    }

    @State private var phase: Phase = .choosing
    @State private var scan: ImportScan?
    @State private var receipt: ImportReceipt?
    @State private var addToGitignore = true
    @State private var undoNote: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            header

            switch phase {
            case .choosing:
                dropZone
            case .review:
                if let scan, scan.isEmpty {
                    nothingFound
                } else {
                    diff
                    guardRails
                }
            case .applying:
                VStack(spacing: Space.m) {
                    ProgressView().controlSize(.small)
                    Text("import.applying")
                        .font(Kind.meta)
                        .foregroundStyle(Tint.ink3)
                }
                .frame(maxWidth: .infinity, minHeight: 220)
            case .done:
                summary
            case .failed(let message):
                notice(message, tone: Tint.amber, wash: Tint.amberWash)
                    .frame(maxWidth: .infinity, minHeight: 220, alignment: .top)
            }

            actions
        }
        .padding(24)
        .frame(width: 640)
        .background(Tint.surface)
        .onAppear {
            if let preview {
                scan = preview
                phase = .review
            } else if let folder {
                begin(folder)
            }
        }
    }

    // MARK: Chrome

    private var header: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("import.title")
                .font(.system(size: 19, weight: .semibold))
                .tracking(Kind.headingTracking)
                .foregroundStyle(Tint.ink)
            Text(subtitle)
                .font(Kind.meta)
                .foregroundStyle(Tint.ink2)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private var subtitle: LocalizedStringKey {
        guard let scan else { return "import.subtitle" }
        return LocalizedStringKey(
            Loc.t("import.found %@", scan.folder.path(percentEncoded: false).abbreviatedHome)
        )
    }

    /// A filled panel rather than the usual dashed rectangle. A dashed border is
    /// the one place every drop target in every app looks the same, and DESIGN.md
    /// §6 puts depth in hairlines — a 1px dash would be the heaviest line in the
    /// product.
    private var dropZone: some View {
        VStack(spacing: Space.m) {
            WardMark(size: .large)
                .fill(Tint.line2)
                .frame(width: 24, height: 44)
            Text("import.dropHint")
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(Tint.ink)
            Text("import.dropBody")
                .font(Kind.meta)
                .foregroundStyle(Tint.ink3)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 380)
            Button("import.choose") { choose() }
                .buttonStyle(GhostButtonStyle())
                .padding(.top, Space.xs)
        }
        .frame(maxWidth: .infinity, minHeight: 260)
        .background(Tint.surface3, in: RoundedRectangle(cornerRadius: Radius.card))
        .overlay(
            RoundedRectangle(cornerRadius: Radius.card)
                .strokeBorder(Tint.line2, lineWidth: 0.5)
        )
        .dropDestination(for: URL.self) { urls, _ in accept(urls) }
    }

    private var nothingFound: some View {
        VStack(spacing: Space.s) {
            StatusDot(status: .protected)
            Text("import.empty.title")
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(Tint.ink)
            Text("import.empty.body")
                .font(Kind.meta)
                .foregroundStyle(Tint.ink3)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 400)
        }
        .frame(maxWidth: .infinity, minHeight: 240)
    }

    // MARK: The diff

    private var diff: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Space.l) {
                ForEach(Array((scan?.files ?? []).enumerated()), id: \.element.id) { index, file in
                    fileCard(index, file)
                }
                agentCard
            }
        }
        // Fixed rather than content-sized: a project with one finding and a project
        // with thirty must put the confirm button in the same place, or the button
        // moves under the cursor between the scan and the click.
        .frame(height: 340)
    }

    private func fileCard(_ index: Int, _ file: ImportFile) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: Space.s) {
                Text(file.path)
                    .font(.system(size: 11.5, design: .monospaced))
                    .foregroundStyle(Tint.ink2)
                Spacer(minLength: Space.s)
                // The guard rail §10.5 asks for, stated on the file it applies to
                // rather than in a paragraph at the bottom nobody reads.
                if !file.ignoredByGit && file.insideProject {
                    tag("import.notIgnored", tone: Tint.amber, wash: Tint.amberWash)
                } else if !file.insideProject {
                    tag("import.outside", tone: Tint.ink3, wash: Tint.surface3)
                }
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 9)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Tint.surface2)

            ForEach(Array(file.findings.enumerated()), id: \.element.id) { position, finding in
                Rectangle().fill(Tint.line).frame(height: 0.5)
                findingRow(file: index, position: position, finding: finding)
            }

            ForEach(file.commands.filter { command in
                file.included.contains { $0.server == command.server }
            }) { command in
                Rectangle().fill(Tint.line).frame(height: 0.5)
                changeRow(
                    line: command.line,
                    before: command.before,
                    after: command.after,
                    note: "import.launch"
                )
                .padding(.leading, 30)
            }
        }
        .background(Tint.surface)
        .clipShape(RoundedRectangle(cornerRadius: Radius.card))
        .overlay(
            RoundedRectangle(cornerRadius: Radius.card)
                .strokeBorder(Tint.line, lineWidth: 0.5)
        )
    }

    private func findingRow(file: Int, position: Int, finding: ImportFinding) -> some View {
        HStack(alignment: .top, spacing: Space.s) {
            Toggle("", isOn: include(file: file, position: position))
                .toggleStyle(.checkbox)
                .labelsHidden()
                // Without this the checkbox arrives in system blue, which is a
                // third accent on a screen whose whole argument is amber → jade.
                .tint(Tint.accent)
                .padding(.top, 1)

            changeRow(
                line: finding.line,
                before: render(finding.key, finding.masked, json: finding.server != nil),
                after: render(finding.key, finding.ref, json: finding.server != nil),
                note: nil
            )
            .opacity(finding.include ? 1 : 0.4)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
    }

    /// Amber above, jade below — the two colours already in the product, carrying
    /// the same meaning here as everywhere else: the value is exposed, then it is
    /// not. A `−`/`+` gutter would say the same thing in a notation borrowed from
    /// a tool this user may never have opened.
    private func changeRow(
        line: Int,
        before: String,
        after: String,
        note: LocalizedStringKey?
    ) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            if let note {
                Text(note)
                    .font(.system(size: 10.5))
                    .foregroundStyle(Tint.ink3)
            }
            row(dot: .shared, text: before, ink: Tint.ink2, line: line)
            row(dot: .protected, text: after, ink: Tint.ink, line: nil)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func row(dot: SecretStatus, text: String, ink: Color, line: Int?) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: Space.s) {
            StatusDot(status: dot)
                .padding(.top, 1)
            Text(line.map { "\($0)" } ?? "")
                .font(.system(size: 10.5, design: .monospaced))
                .monospacedDigit()
                .foregroundStyle(Tint.ink3)
                .frame(width: 22, alignment: .trailing)
            Text(text)
                .font(.system(size: 11.5, design: .monospaced))
                .foregroundStyle(ink)
                .lineLimit(1)
                .truncationMode(.middle)
            Spacer(minLength: 0)
        }
    }

    private func render(_ key: String, _ value: String, json: Bool) -> String {
        json ? "\"\(key)\": \"\(value)\"" : "\(key)=\(value)"
    }

    private var agentCard: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: Space.s) {
                Text(scan?.agent.url.lastPathComponent ?? "AGENTS.md")
                    .font(.system(size: 11.5, design: .monospaced))
                    .foregroundStyle(Tint.ink2)
                Spacer(minLength: Space.s)
                if scan?.agent.isNew == true {
                    tag("import.newFile", tone: Tint.ink3, wash: Tint.surface3)
                }
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 9)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Tint.surface2)

            Rectangle().fill(Tint.line).frame(height: 0.5)

            HStack(alignment: .firstTextBaseline, spacing: Space.s) {
                StatusDot(status: .protected)
                Text("import.agentBlock")
                    .font(Kind.meta)
                    .foregroundStyle(Tint.ink2)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 11)
        }
        .background(Tint.surface)
        .clipShape(RoundedRectangle(cornerRadius: Radius.card))
        .overlay(
            RoundedRectangle(cornerRadius: Radius.card)
                .strokeBorder(Tint.line, lineWidth: 0.5)
        )
    }

    /// The two promises §10.5 makes on this screen's behalf, where the user is
    /// looking when they decide.
    private var guardRails: some View {
        VStack(alignment: .leading, spacing: Space.s) {
            if let exposed = scan?.exposedFiles, !exposed.isEmpty {
                notice(
                    Loc.t("import.exposed %@", exposed.map(\.path).joined(separator: ", ")),
                    tone: Tint.amber,
                    wash: Tint.amberWash
                )
                Toggle(isOn: $addToGitignore) {
                    Text("import.addToGitignore")
                        .font(Kind.meta)
                        .foregroundStyle(Tint.ink2)
                }
                .toggleStyle(.checkbox)
                .tint(Tint.accent)
            }
            HStack(spacing: 6) {
                Image(systemName: "arrow.uturn.backward")
                    .font(.system(size: 10))
                    .foregroundStyle(Tint.ink3)
                Text("import.backupNote")
                    .font(Kind.meta)
                    .foregroundStyle(Tint.ink3)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private var summary: some View {
        VStack(alignment: .leading, spacing: Space.m) {
            HStack(spacing: Space.s) {
                StatusDot(status: .protected)
                Text("import.done.title")
                    .font(.system(size: 15, weight: .medium))
                    .foregroundStyle(Tint.ink)
            }
            if let receipt {
                Text("import.done.body \(receipt.addedSecrets.count) \(receipt.rewrittenCount)")
                    .font(Kind.meta)
                    .foregroundStyle(Tint.ink2)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if let undoNote {
                notice(undoNote, tone: Tint.amber, wash: Tint.amberWash)
            }
        }
        .frame(maxWidth: .infinity, minHeight: 180, alignment: .topLeading)
        .padding(.top, Space.s)
    }

    private func notice(_ text: String, tone: Color, wash: Color) -> some View {
        Text(text)
            .font(Kind.meta)
            .foregroundStyle(tone)
            .fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, Space.m)
            .padding(.vertical, Space.s)
            .background(wash, in: RoundedRectangle(cornerRadius: Radius.card))
    }

    private func tag(_ key: LocalizedStringKey, tone: Color, wash: Color) -> some View {
        Text(key)
            .font(.system(size: 10.5))
            .foregroundStyle(tone)
            .padding(.horizontal, 7)
            .padding(.vertical, 2)
            .background(wash, in: RoundedRectangle(cornerRadius: 5))
    }

    // MARK: Actions

    @ViewBuilder
    private var actions: some View {
        HStack(spacing: Space.s) {
            Spacer()
            switch phase {
            case .done:
                Button("import.undo") { undo() }
                    .buttonStyle(GhostButtonStyle())
                    .disabled(receipt == nil)
                Button("action.done") { dismiss() }
                    .buttonStyle(PrimaryButtonStyle())
                    .keyboardShortcut(.defaultAction)
            case .review where scan?.isEmpty == false:
                Button("action.cancel") { dismiss() }
                    .buttonStyle(GhostButtonStyle())
                    .keyboardShortcut(.cancelAction)
                Button("import.confirm \(scan?.includedCount ?? 0)") { apply() }
                    .buttonStyle(PrimaryButtonStyle())
                    .keyboardShortcut(.defaultAction)
                    .disabled((scan?.includedCount ?? 0) == 0)
            case .applying:
                Button("action.cancel") {}
                    .buttonStyle(GhostButtonStyle())
                    .disabled(true)
            default:
                Button("action.done") { dismiss() }
                    .buttonStyle(GhostButtonStyle())
                    .keyboardShortcut(.cancelAction)
            }
        }
    }

    private func include(file: Int, position: Int) -> Binding<Bool> {
        Binding(
            get: { scan?.files[file].findings[position].include ?? false },
            set: { scan?.files[file].findings[position].include = $0 }
        )
    }

    private func begin(_ url: URL) {
        scan = ImportScanner.scan(folder: url)
        phase = .review
    }

    private func accept(_ urls: [URL]) -> Bool {
        // Dropping a file inside a project is what people do when they mean the
        // project. Walking up to its folder is friendlier than refusing the drop.
        guard let url = urls.first else { return false }
        var isDirectory: ObjCBool = false
        FileManager.default.fileExists(atPath: url.path(percentEncoded: false), isDirectory: &isDirectory)
        begin(isDirectory.boolValue ? url : url.deletingLastPathComponent())
        return true
    }

    private func choose() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.prompt = Loc.t("import.choose")
        guard panel.runModal() == .OK, let url = panel.url else { return }
        begin(url)
    }

    private func apply() {
        guard let scan else { return }
        let paths = addToGitignore ? scan.exposedFiles.map(\.path) : []
        phase = .applying
        Task {
            switch await store.runImport(scan, addingToGitignore: paths) {
            case .success(let done):
                receipt = done
                phase = .done
            case .failure(let error):
                phase = .failed(Loc.t("import.failed %@", error.localizedDescription))
            }
        }
    }

    private func undo() {
        guard let receipt else { return }
        Task {
            let problems = await store.undoImport(receipt)
            undoNote = problems.isEmpty
                ? Loc.t("import.undone")
                : Loc.t("import.undoProblems %@", problems.joined(separator: ", "))
            self.receipt = nil
        }
    }
}

extension String {
    /// `/Users/me/code/shop` → `~/code/shop`. The home prefix is the least
    /// informative part of every path this screen shows.
    var abbreviatedHome: String {
        let home = NSHomeDirectory()
        return hasPrefix(home) ? "~" + String(dropFirst(home.count)) : self
    }
}
