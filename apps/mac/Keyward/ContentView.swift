//
//  ContentView.swift
//  The list, the detail, and the add sheet — the whole app.
//

import SwiftUI

struct ContentView: View {
    /// The popover is the same list, condensed — 22pt avatars, no masked value,
    /// no chevron (DESIGN.md §4). One view, two densities, so the two surfaces
    /// cannot drift apart.
    var compact = false

    @Environment(Store.self) private var store
    @Environment(Prefs.self) private var prefs
    @State private var hovered: String?
    @State private var showingAdd = false
    @State private var showingSettings = false
    @State private var showingUseWithAI = false
    @State private var showingImport = false
    @State private var droppedFolder: URL?
    @State private var query = ""
    @FocusState private var searchFocused: Bool

    var body: some View {
        Group {
            if compact {
                // The menu bar has no room for two panes; tapping a row there
                // opens the main window instead of pushing a detail view.
                column
            } else {
                // The list column runs the full height of the window with the
                // traffic lights over it, and the toolbar covers only the detail
                // side — the left-right split, not a band across the top of both.
                NavigationSplitView {
                    column
                        .navigationSplitViewColumnWidth(min: 250, ideal: 286, max: 360)
                        // The system toggle is oversized beside our own controls,
                        // and this sidebar is never hidden.
                        .toolbar(removing: .sidebarToggle)
                } detail: {
                    detailPane
                        .toolbar {
                            ToolbarItemGroup(placement: .primaryAction) {
                                Spacer()
                                useWithAIButton
                                addButton
                                    .padding(.trailing, 12)
                            }
                        }
                }
                .navigationSplitViewStyle(.balanced)
            }
        }
        // No tall minimum in the popover: it is sized to its content, and a
        // floor here would defeat that.
        .frame(minWidth: compact ? 296 : 640)
        .frame(minHeight: compact ? 180 : 460)
        .task { await store.load() }
        .task { await store.followLiveSessions() }
        // Every sheet carries the locale explicitly.
        //
        // A sheet is presented in its own window on macOS and does not inherit
        // the `\.locale` the scene set, so `Text("use.title")` resolved against
        // the system language while `Loc.t(...)` next to it resolved against the
        // user's choice — half an English dialog inside a Chinese app. The
        // sidebar looked right the whole time, which is why it survived.
        .sheet(isPresented: $showingAdd) { AddView().localised(prefs) }
        .sheet(isPresented: $showingUseWithAI) { UseWithAISheet().localised(prefs) }
        // The whole window is the drop target, not a strip of it: a user dragging
        // a folder at an app they opened ten seconds ago aims at the app, and a
        // drop that lands 20pt outside a zone reads as "Keyward doesn't do that".
        .dropDestination(for: URL.self) { urls, _ in
            guard !compact, let url = urls.first else { return false }
            droppedFolder = folder(containing: url)
            showingImport = true
            return true
        }
        .onReceive(NotificationCenter.default.publisher(for: .keywardImport)) { _ in
            // The popover shares this view; opening a 640pt sheet out of a 296pt
            // popover puts it somewhere the user did not point at.
            guard !compact else { return }
            droppedFolder = nil
            showingImport = true
        }
        .sheet(isPresented: $showingImport) {
            ImportView(folder: droppedFolder)
                // A new folder means a new scan, and SwiftUI reuses a sheet's
                // state across presentations unless told the identity changed.
                .id(droppedFolder?.path ?? "picker")
                .localised(prefs)
        }
        .sheet(isPresented: $showingSettings) {
            VStack(alignment: .trailing, spacing: 0) {
                SettingsView()
                Button("action.done") { showingSettings = false }
                    .keyboardShortcut(.defaultAction)
                    .padding([.trailing, .bottom], Space.xl)
            }
            .localised(prefs)
        }
    }

    /// Bring the main window back from the popover, through the same path the
    /// Dock icon uses.
    static func openMainWindow() {
        (NSApp.delegate as? AppDelegate)?.showMainWindow()
    }



    /// The chrome the two secondary toolbar items share. `GhostButtonStyle` is
    /// sized for a sheet's button row and reads as oversized up here.
    private func chip(_ icon: String, _ title: LocalizedStringKey) -> some View {
        HStack(spacing: 5) {
            Image(systemName: icon)
                .font(.system(size: 11, weight: .medium))
            Text(title)
                .font(.system(size: 13))
        }
        .foregroundStyle(Tint.ink)
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .background(
            RoundedRectangle(cornerRadius: Radius.control).fill(Tint.surface)
        )
        .overlay(
            RoundedRectangle(cornerRadius: Radius.control)
                .strokeBorder(Tint.line2, lineWidth: 0.5)
        )
        .shadow(color: .black.opacity(0.05), radius: 1, y: 0.5)
    }

    /// A dropped `.env` means the project it sits in.
    private func folder(containing url: URL) -> URL {
        var isDirectory: ObjCBool = false
        FileManager.default.fileExists(
            atPath: url.path(percentEncoded: false), isDirectory: &isDirectory
        )
        return isDirectory.boolValue ? url : url.deletingLastPathComponent()
    }

    /// How to get an agent using this. Secondary to `Add secret` and next to it:
    /// storing a key and teaching your agent about it are the two things a person
    /// does once, and the second is useless without the first.
    private var useWithAIButton: some View {
        Button {
            showingUseWithAI = true
        } label: {
            HStack(spacing: 5) {
                Image(systemName: "sparkles")
                    .font(.system(size: 11, weight: .medium))
                Text("use.button")
                    .font(.system(size: 13))
            }
            .foregroundStyle(Tint.ink)
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .background(RoundedRectangle(cornerRadius: Radius.control).fill(Tint.surface))
            .overlay(
                RoundedRectangle(cornerRadius: Radius.control)
                    .strokeBorder(Tint.line2, lineWidth: 0.5)
            )
            .shadow(color: .black.opacity(0.05), radius: 1, y: 0.5)
        }
        .buttonStyle(.plain)
        .padding(.trailing, 10)
    }

    /// The primary action, in the toolbar rather than a footer (DESIGN.md §4).
    private var addButton: some View {
        Button {
            showingAdd = true
        } label: {
            HStack(spacing: 5) {
                Image(systemName: "plus")
                    .font(.system(size: 11, weight: .bold))
                Text("action.addSecret.short")
                    .font(.system(size: 13, weight: .medium))
            }
            .foregroundStyle(Tint.onAccent)
            .padding(.horizontal, 13)
            .padding(.vertical, 6)
            .background(
                RoundedRectangle(cornerRadius: Radius.control)
                    .fill(LinearGradient(
                        colors: [Tint.accentTop, Tint.accentBottom],
                        startPoint: .top, endPoint: .bottom
                    ))
            )
            .overlay(
                RoundedRectangle(cornerRadius: Radius.control)
                    .strokeBorder(.white.opacity(0.22), lineWidth: 0.5)
            )
            .shadow(color: .black.opacity(0.14), radius: 1.5, y: 1)
        }
        .buttonStyle(.plain)
        .keyboardShortcut("n", modifiers: .command)
    }

    /// The list column. In the popover it also carries the search field and the
    /// add button, since there is no toolbar there.
    private var column: some View {
        VStack(spacing: 0) {
            // Search filters this list, so it lives in this column — not in the
            // toolbar over the pane beside it, where it belonged to nothing.
            searchBar
                .padding(.horizontal, Space.m)
                .padding(.bottom, Space.s)
            list
            Divider().overlay(Tint.line)
            footer
        }
        .background(compact ? Tint.surface : Tint.surface2)
    }

    /// Detail beside the list rather than pushed over it. Replacing the list on
    /// selection is an iOS navigation idiom; on a desktop window it hides the
    /// thing you are navigating and costs a trip back for every comparison.
    @ViewBuilder
    private var detailPane: some View {
        if let selected = store.selected {
            DetailView(secret: selected)
        } else if store.secrets.isEmpty, case .ready = store.phase {
            gettingStarted
        } else {
            VStack(spacing: Space.m) {
                WardMark(size: .large)
                    .fill(Tint.line2)
                    .frame(width: 28, height: 52)
                Text("detail.empty")
                    .font(Kind.meta)
                    .foregroundStyle(Tint.ink3)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Tint.surface)
        }
    }

    /// Shown when the vault is empty — the one moment a new user is guaranteed to
    /// be looking at this pane and has nothing else to read.
    ///
    /// Replaces a toolbar button that opened a paragraph to copy into a file.
    /// Deleting that button without this was a regression: it was the only place
    /// in the app that explained what any of this is for.
    private var gettingStarted: some View {
        VStack(alignment: .leading, spacing: 22) {
            VStack(alignment: .leading, spacing: 6) {
                WardMark(size: .large)
                    .fill(Tint.ink)
                    .frame(width: 20, height: 37)
                    .padding(.bottom, 6)
                Text("start.title")
                    .font(.system(size: 20, weight: .semibold))
                    .tracking(Kind.headingTracking)
                Text("start.subtitle")
                    .font(Kind.meta)
                    .foregroundStyle(Tint.ink2)
                    .fixedSize(horizontal: false, vertical: true)
            }

            VStack(alignment: .leading, spacing: 14) {
                step(1, "start.step1")
                step(2, "start.step2")
                step(3, "start.step3")
            }

            Button("start.openUse") { showingUseWithAI = true }
                .buttonStyle(GhostButtonStyle())
        }
        .frame(maxWidth: 380, alignment: .leading)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(28)
        .background(Tint.surface)
    }

    private func step(_ number: Int, _ text: LocalizedStringKey) -> some View {
        HStack(alignment: .top, spacing: 11) {
            Text("\(number)")
                .font(.system(size: 11, weight: .semibold, design: .monospaced))
                .foregroundStyle(Tint.ink3)
                .frame(width: 20, height: 20)
                .background(Tint.surface3, in: Circle())
            Text(text)
                .font(Kind.body)
                .foregroundStyle(Tint.ink)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private var searchBar: some View {
        HStack(spacing: 7) {
            Image(systemName: "magnifyingglass")
                .font(.system(size: 11, weight: .medium))
                .foregroundStyle(Tint.ink3)
            TextField("search.placeholder", text: $query)
                .textFieldStyle(.plain)
                .font(Kind.body)
                .focused($searchFocused)
            if !searchFocused && query.isEmpty {
                Text(verbatim: "⌘F")
                    .font(.system(size: 10.5))
                    .foregroundStyle(Tint.ink3)
                    .padding(.horizontal, 5)
                    .padding(.vertical, 1)
                    .background(
                        RoundedRectangle(cornerRadius: Radius.key)
                            .fill(Tint.surface)
                            .overlay(
                                RoundedRectangle(cornerRadius: Radius.key)
                                    .strokeBorder(Tint.line2, lineWidth: 0.5)
                            )
                    )
            }
        }
        .padding(.horizontal, Space.s)
        .padding(.vertical, 5)
        .background(Tint.surface3, in: RoundedRectangle(cornerRadius: Radius.input))
        .overlay(
            RoundedRectangle(cornerRadius: Radius.input)
                .strokeBorder(searchFocused ? Tint.accent : Tint.line2, lineWidth: searchFocused ? 1.5 : 0.5)
        )
        .background(
            RoundedRectangle(cornerRadius: Radius.input)
                .fill(Tint.accentWash)
                .opacity(searchFocused ? 1 : 0)
                .padding(-3)
        )
        .background {
            // Invisible button carrying the shortcut, so ⌘F is real rather than
            // a keycap that lies about what the app does.
            Button("") { searchFocused = true }
                .keyboardShortcut("f", modifiers: .command)
                .opacity(0)
        }
    }

    private var filtered: [SecretSummary] {
        guard !query.isEmpty else { return store.secrets }
        return store.secrets.filter {
            $0.display.localizedCaseInsensitiveContains(query)
                || $0.name.localizedCaseInsensitiveContains(query)
        }
    }

    @ViewBuilder
    private var list: some View {
        switch store.phase {
        case .loading, .idle:
            centred { ProgressView().controlSize(.small) }
        case .failed(let message) where store.secrets.isEmpty:
            centred {
                VStack(spacing: Space.s) {
                    WardMark(size: .large)
                        .fill(Tint.line2)
                        .frame(width: 22, height: 34)
                    Text(message)
                        .font(Kind.meta)
                        .foregroundStyle(Tint.ink3)
                        .multilineTextAlignment(.center)
                        .padding(.horizontal, Space.xl)
                    Button("action.retry") { Task { await store.load() } }
                        .buttonStyle(.link)
                        .font(Kind.meta)
                }
            }
        default:
            ScrollView {
                VStack(spacing: 0) {
                    ForEach(filtered) { secret in
                        rowButton(secret)
                    }
                }
            }
        }
    }

    private func rowButton(_ secret: SecretSummary) -> some View {
        Button {
            Task { await store.select(secret) }
            // The popover has no detail pane, so a tap there means "show me this"
            // — which is the window's job. Without this the row simply did
            // nothing, which reads as a broken control rather than a compact one.
            if compact {
                Self.openMainWindow()
            }
        } label: {
            SecretRow(
                secret: secret,
                compact: true,
                isHovered: hovered == secret.id,
                isSelected: store.selected?.id == secret.id
            )
        }
        .buttonStyle(.plain)
        .background {
            let isSelected = store.selected?.id == secret.id
            RoundedRectangle(cornerRadius: Radius.avatar)
                .fill(isSelected ? Tint.selected : (hovered == secret.id ? Tint.hover : .clear))
                .overlay(
                    RoundedRectangle(cornerRadius: Radius.avatar)
                        .strokeBorder(isSelected ? Tint.line2 : .clear, lineWidth: 0.5)
                )
                .padding(.horizontal, 6)
                .padding(.vertical, 1)
        }
        .overlay(alignment: .top) {
            // Separator inset past the avatar, and suppressed around the hovered
            // and selected rows so their fills read as one block (DESIGN.md §4).
            if secret.id != filtered.first?.id, !touchesHighlight(secret) {
                Rectangle()
                    .fill(Tint.line)
                    .frame(height: 0.5)
                    .padding(.leading, 52)
            }
        }
        .onHover { hovered = $0 ? secret.id : (hovered == secret.id ? nil : hovered) }
        .animation(.easeOut(duration: 0.13), value: hovered)
    }

    /// Whether this row or the one above it is highlighted, so the separator
    /// between them can be suppressed.
    private func touchesHighlight(_ secret: SecretSummary) -> Bool {
        let marked: (String) -> Bool = { $0 == hovered || $0 == store.selected?.id }
        guard let index = filtered.firstIndex(where: { $0.id == secret.id }) else { return false }
        if marked(secret.id) { return true }
        let above = filtered.index(before: index)
        return filtered.indices.contains(above) && marked(filtered[above].id)
    }

    private var footer: some View {
        HStack {
            if compact {
                Button {
                    showingAdd = true
                } label: {
                    Text("action.addSecret")
                        .font(Kind.body)
                        .fontWeight(.medium)
                        .foregroundStyle(Tint.accent)
                }
                .buttonStyle(.plain)
            }

            Spacer()

            if store.liveCount > 0 {
                HStack(spacing: 5) {
                    LiveDot()
                    Text("footer.live \(store.liveCount)")
                        .font(Kind.meta)
                        .foregroundStyle(Tint.jade)
                }
                .lineLimit(1)
            } else {
                Text("footer.short \(store.secrets.count)")
                    .font(Kind.meta)
                    .foregroundStyle(Tint.ink3)
                    .lineLimit(1)
            }

            Button {
                showingSettings = true
            } label: {
                Image(systemName: "gearshape")
                    .font(.system(size: 12))
                    .foregroundStyle(Tint.ink3)
            }
            .buttonStyle(.plain)
            .accessibilityLabel(Text("settings.title"))
        }
        .padding(.horizontal, Space.m)
        .padding(.vertical, Space.s)
        .background(Tint.surface2)
    }

    private func centred<V: View>(@ViewBuilder _ content: () -> V) -> some View {
        VStack { Spacer(); content(); Spacer() }
            .frame(maxWidth: .infinity)
    }
}

struct DetailView: View {
    @Environment(Store.self) private var store
    @Environment(Prefs.self) private var prefs
    let secret: SecretSummary

    @State private var copied = false
    @State private var rotating = false
    @State private var newValue = ""
    @State private var confirmingDelete = false
    @State private var draftName = ""
    @State private var editingName = false
    @State private var error: String?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 32) {
                header

                if let error {
                    Text(error)
                        .font(Kind.meta)
                        .foregroundStyle(Tint.amber)
                        .padding(.horizontal, Space.m)
                        .padding(.vertical, Space.s)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(Tint.amberWash, in: RoundedRectangle(cornerRadius: Radius.card))
                }

                if let live = secret.live {
                    liveCard(live)
                }

                fields

                // One line, right under the reference it is about. A user who has
                // just added a key is holding `keyward://openai` and no idea what
                // to do with it; this is the sentence that closes that gap.
                Text(Loc.t("detail.how %@", secret.name))
                .font(Kind.meta)
                .foregroundStyle(Tint.ink2)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.top, -18)

                if store.activity.filter({ $0 > 0 }).count >= 3 {
                    section("detail.fortnight", trailing: "detail.uses \(store.uses.count)") {
                        ActivityStrip(counts: store.activity)
                            .frame(height: 38)
                    }
                }

                section("detail.usedBy") {
                    if store.uses.isEmpty {
                        Text("detail.noUses")
                            .font(Kind.meta)
                            .foregroundStyle(Tint.ink3)
                    } else {
                        usesTable
                    }
                }
            }
            .padding(.horizontal, 28)
            .padding(.top, 26)
            .padding(.bottom, 36)
            .frame(maxWidth: 560, alignment: .leading)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Tint.surface)
        // Rebuild the scroll view when the selection changes, so a long usage
        // table does not leave the next secret opened halfway down.
        .id(secret.name)
        .sheet(isPresented: $rotating) { rotateSheet.localised(prefs) }
        .confirmationDialog(
            Text("delete.title \(secret.display)"),
            isPresented: $confirmingDelete,
            titleVisibility: .visible
        ) {
            Button("detail.delete", role: .destructive) {
                Task { error = await store.remove(secret) }
            }
            Button("action.cancel", role: .cancel) {}
        } message: {
            Text("delete.message")
        }
        // Reset per-secret editing state when the selection changes, so a rename
        // begun on one secret does not appear half-open on the next.
        .onChange(of: secret.name) {
            editingName = false
            error = nil
            newValue = ""
        }
    }

    private var rotateSheet: some View {
        SheetFrame(title: "rotate.title \(secret.display)") {
            FormField(label: "detail.secret", hint: "rotate.hint") {
                KeyTextField(text: $newValue, secure: true, mono: true)
            }
        } actions: {
            Button("action.cancel") {
                rotating = false
                newValue = ""
            }
            .buttonStyle(GhostButtonStyle())
            .keyboardShortcut(.cancelAction)

            Button("rotate.confirm") {
                let value = newValue
                newValue = ""
                rotating = false
                Task { error = await store.rotate(secret, value: value) }
            }
            .buttonStyle(PrimaryButtonStyle())
            .keyboardShortcut(.defaultAction)
            .disabled(newValue.isEmpty)
        }
    }

    private func commitRename() {
        let name = draftName.trimmingCharacters(in: .whitespacesAndNewlines)
        editingName = false
        guard !name.isEmpty, name != secret.display else { return }
        Task { error = await store.rename(secret, to: name) }
    }

    private var header: some View {
        HStack(alignment: .center, spacing: 14) {
            Avatar(letter: secret.letter, hex: secret.tint, logo: secret.logo, side: 52)

            VStack(alignment: .leading, spacing: 3) {
                // Editable in place. Renaming is the one edit people expect to be
                // free, and a sheet for a single text field is ceremony.
                if editingName {
                    TextField("", text: $draftName)
                        .textFieldStyle(.plain)
                        .font(Kind.heading(22))
                        .padding(.horizontal, 8)
                        .padding(.vertical, 4)
                        .background(
                            RoundedRectangle(cornerRadius: Radius.input).fill(Tint.surface3)
                        )
                        .overlay(
                            RoundedRectangle(cornerRadius: Radius.input)
                                .strokeBorder(Tint.accent.opacity(0.6), lineWidth: 1)
                        )
                        .frame(maxWidth: 280)
                        .onSubmit { commitRename() }
                } else {
                    Text(secret.display)
                        .font(Kind.heading(26))
                        .tracking(Kind.headingTracking)
                        .onTapGesture {
                            draftName = secret.display
                            editingName = true
                        }
                }

                // The dot alone was an unlabelled speck floating beside the menu.
                // Paired with its word it becomes the one thing worth reading here.
                HStack(spacing: 6) {
                    StatusDot(status: secret.status)
                    Text(secret.status.label)
                        .font(Kind.meta)
                        .foregroundStyle(Tint.ink2)
                }
            }

            Spacer(minLength: Space.m)

            Menu {
                Button("detail.rename") {
                    draftName = secret.display
                    editingName = true
                }
                Button("detail.replace") { rotating = true }
                Divider()
                Button("detail.delete", role: .destructive) { confirmingDelete = true }
            } label: {
                Image(systemName: "ellipsis")
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(Tint.ink2)
                    .frame(width: 28, height: 24)
                    .background(Tint.surface3, in: RoundedRectangle(cornerRadius: Radius.control))
                    .overlay(
                        RoundedRectangle(cornerRadius: Radius.control)
                            .strokeBorder(Tint.line2, lineWidth: 0.5)
                    )
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .fixedSize()
        }
    }

    /// Shown only while a session is open. It is the one place the product's
    /// central claim is visible as it happens, so it says what is true — the
    /// program is talking through Keyward and holds nothing.
    private func liveCard(_ live: LiveSession) -> some View {
        HStack(spacing: 12) {
            LiveDot()
            VStack(alignment: .leading, spacing: 2) {
                Text("live.title")
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(Tint.ink)
                Text(
                    live.actor.map { Loc.t("live.detail %@ %lld", $0, live.requests) }
                        ?? Loc.t("live.detailNoActor %lld", live.requests)
                )
                .font(Kind.meta)
                .foregroundStyle(Tint.ink2)
            }
            Spacer(minLength: Space.s)
            Button("live.stop") {
                Task { await store.stopSession(live.session) }
            }
            .buttonStyle(GhostButtonStyle())
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .background(Tint.jadeWash, in: RoundedRectangle(cornerRadius: Radius.card))
        .overlay(
            RoundedRectangle(cornerRadius: Radius.card)
                .strokeBorder(Tint.jade2.opacity(0.28), lineWidth: 0.5)
        )
    }

    private var fields: some View {
        VStack(spacing: 0) {
            field(
                label: "detail.secret",
                value: secret.masked ?? "———",
                actionTitle: "detail.replace"
            ) { rotating = true }

            Rectangle().fill(Tint.line).frame(height: 0.5)

            field(
                label: "detail.reference",
                value: secret.ref,
                actionTitle: copied ? "action.copied" : "action.copy"
            ) {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(secret.ref, forType: .string)
                copied = true
                Task {
                    try? await Task.sleep(for: .seconds(1.3))
                    copied = false
                }
            }
        }
        .background(Tint.surface)
        .clipShape(RoundedRectangle(cornerRadius: Radius.card))
        .overlay(
            RoundedRectangle(cornerRadius: Radius.card)
                .strokeBorder(Tint.line, lineWidth: 0.5)
        )
    }

    /// Label above, value below — not side by side.
    ///
    /// Sharing one line made the label, the value and the action fight for the
    /// same horizontal space, and the value lost: `postgres://app:…@db/app`
    /// truncated to `postgre…/app` in a pane with 400pt to spare.
    private func field(
        label: LocalizedStringKey,
        value: String,
        actionTitle: LocalizedStringKey,
        action: @escaping () -> Void
    ) -> some View {
        HStack(alignment: .center, spacing: Space.m) {
            VStack(alignment: .leading, spacing: 4) {
                // `ink2`, not the accent. An earlier version coloured these
                // labels to give each field a scannable anchor — borrowed from
                // 1Password, where the accent is a brand colour. Ours became
                // graphite, so a coloured label would now be the *status* colour
                // saying something that has nothing to do with status.
                Text(label)
                    .font(.system(size: 11.5, weight: .medium))
                    .foregroundStyle(Tint.ink2)
                Text(value)
                    .font(.system(size: 14, design: .monospaced))
                    .foregroundStyle(Tint.ink)
                    .textSelection(.enabled)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
            Spacer(minLength: Space.s)
            Button(actionTitle, action: action)
                .buttonStyle(.plain)
                .font(Kind.meta)
                .fontWeight(.medium)
                .foregroundStyle(Tint.ink)
                .padding(.horizontal, 11)
                .padding(.vertical, 5)
                .background(Tint.surface3, in: RoundedRectangle(cornerRadius: Radius.control))
                .overlay(
                    RoundedRectangle(cornerRadius: Radius.control)
                        .strokeBorder(Tint.line2, lineWidth: 0.5)
                )
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 12)
    }

    private func section<Body: View>(
        _ title: LocalizedStringKey,
        trailing: LocalizedStringKey? = nil,
        @ViewBuilder content: () -> Body
    ) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .firstTextBaseline, spacing: Space.s) {
                Text(title)
                    .font(.system(size: 11, weight: .semibold))
                    .tracking(0.66)
                    .textCase(.uppercase)
                    .foregroundStyle(Tint.ink3)
                if let trailing {
                    Text(trailing)
                        .font(Kind.meta)
                        .foregroundStyle(Tint.ink3)
                }
            }
            content()
        }
    }

    /// Four columns — time, who, project, how — per the design. The "how" column
    /// is the one that matters: it is where a user learns that "brokered" means
    /// the program never held the value.
    private var usesTable: some View {
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                header("table.time", width: 92)
                header("table.who", width: 104)
                header("table.project", width: 92)
                header("table.how", width: nil)
            }
            .padding(.horizontal, Space.m)
            .padding(.vertical, 6)
            .background(Tint.surface2)

            ForEach(store.uses) { use in
                HStack(spacing: 0) {
                    Text(use.at.relative)
                        .font(.system(size: 11, design: .monospaced))
                        .monospacedDigit()
                        .foregroundStyle(Tint.ink3)
                        .frame(width: 92, alignment: .leading)

                    Text(use.actor)
                        .font(Kind.meta)
                        .foregroundStyle(Tint.ink)
                        .lineLimit(1)
                        .frame(width: 104, alignment: .leading)

                    Group {
                        if let project = use.project {
                            Text(project)
                                .font(.system(size: 10.5))
                                .foregroundStyle(Tint.ink2)
                                .padding(.horizontal, 6)
                                .padding(.vertical, 1)
                                .background(
                                    RoundedRectangle(cornerRadius: 5).fill(Tint.surface3)
                                )
                        }
                    }
                    .frame(width: 92, alignment: .leading)

                    HStack(spacing: 6) {
                        Circle()
                            .fill(use.wasBrokered ? Tint.jade2 : Tint.amber)
                            .frame(width: 5, height: 5)
                        Text(use.wasBrokered ? "how.brokered" : "how.handed")
                            .font(Kind.meta)
                            .foregroundStyle(Tint.ink2)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .padding(.horizontal, Space.m)
                .padding(.vertical, 7)
                .overlay(alignment: .top) {
                    Rectangle().fill(Tint.line).frame(height: 0.5)
                }
            }
        }
        .background(Tint.surface)
        .clipShape(RoundedRectangle(cornerRadius: Radius.card))
        .overlay(
            RoundedRectangle(cornerRadius: Radius.card)
                .strokeBorder(Tint.line, lineWidth: 0.5)
        )
    }

    private func header(_ key: LocalizedStringKey, width: CGFloat?) -> some View {
        Text(key)
            .font(.system(size: 10.5, weight: .medium))
            .tracking(0.5)
            .textCase(.uppercase)
            .foregroundStyle(Tint.ink3)
            .frame(width: width, alignment: .leading)
            .frame(maxWidth: width == nil ? .infinity : nil, alignment: .leading)
    }
}

/// Fourteen bars, one per day. Empty days keep a 2pt stub in `line2` rather than
/// vanishing — a gap that reads as missing data is worse than one that reads as
/// zero.
struct ActivityStrip: View {
    let counts: [Int]

    var body: some View {
        GeometryReader { geo in
            let peak = max(counts.max() ?? 1, 1)
            HStack(alignment: .bottom, spacing: 3) {
                ForEach(Array(counts.enumerated()), id: \.offset) { _, count in
                    RoundedRectangle(cornerRadius: 2)
                        .fill(count == 0 ? Tint.line2 : Tint.jade2)
                        .opacity(count == 0 ? 0.35 : 0.85)
                        // A day with one use gets a stub tall enough to read as a
                        // bar; an empty day gets a 2pt tick that reads as a
                        // baseline. Scaling both from the same peak made the
                        // empty days as loud as the data.
                        .frame(height: count == 0
                            ? 2
                            : max(8, geo.size.height * CGFloat(count) / CGFloat(peak)))
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottom)
        }
    }
}

struct AddView: View {
    @Environment(Store.self) private var store
    @Environment(\.dismiss) private var dismiss

    @State private var display = ""
    @State private var value = ""
    @State private var upstream = ""
    @State private var showingUpstream = false
    @State private var error: String?

    private var slug: String? { Slug.make(from: display) }

    var body: some View {
        SheetFrame(title: "add.title") {
            FormField(label: "add.name") {
                KeyTextField(text: $display)
            }

            FormField(label: "add.value", hint: "add.valueHint") {
                KeyTextField(text: $value, secure: true, mono: true)
            }

            // Known services are forwarded from a table in the daemon; this is
            // the escape hatch for everything else. Collapsed by default because
            // the answer is "leave it alone" for almost every secret, and an
            // always-visible URL field invites people to fill it in wrongly.
            if showingUpstream {
                FormField(label: "add.upstream", hint: "add.upstreamHint") {
                    KeyTextField(text: $upstream, mono: true)
                }
            } else {
                Button("add.upstreamReveal") { showingUpstream = true }
                    .buttonStyle(.plain)
                    .font(Kind.meta)
                    .foregroundStyle(Tint.ink2)
            }

            FormField(label: "add.reference", hint: "add.referenceHint") {
                KeyTextField(
                    text: .constant(slug.map { "keyward://\($0)" }
                        ?? Loc.t("add.referencePending")),
                    mono: true,
                    readOnly: true
                )
            }

            if let error {
                Text(error)
                    .font(Kind.meta)
                    .foregroundStyle(Tint.amber)
                    .fixedSize(horizontal: false, vertical: true)
            }
        } actions: {
            Button("action.cancel") { dismiss() }
                .buttonStyle(GhostButtonStyle())
                .keyboardShortcut(.cancelAction)
            Button("action.add") {
                Task {
                    error = await store.add(
                        display: display,
                        value: value,
                        upstream: upstream.isEmpty ? nil : upstream
                    )
                    if error == nil { dismiss() }
                }
            }
            .buttonStyle(PrimaryButtonStyle())
            .keyboardShortcut(.defaultAction)
            .disabled(slug == nil || value.isEmpty)
        }
    }
}
