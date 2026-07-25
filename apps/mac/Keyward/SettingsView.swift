//
//  SettingsView.swift
//  Appearance, language, login item. Three rows — the settings screen of a tool
//  that is opened for ten seconds at a time (DESIGN.md §1).
//

import SwiftUI

struct SettingsView: View {
    @Environment(Prefs.self) private var prefs
    @State private var loginError: String?
    @State private var cliState = CLIInstall.state()
    @State private var cliError: String?

    var body: some View {
        @Bindable var prefs = prefs

        VStack(alignment: .leading, spacing: Space.l) {
            VStack(spacing: 0) {
                row(
                    title: "settings.appearance",
                    detail: "settings.appearanceHint"
                ) {
                    Picker("", selection: $prefs.appearance) {
                        ForEach(Prefs.Appearance.allCases) { Text($0.label).tag($0) }
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                    .fixedSize()
                    .tint(Tint.accent)
                }

                Rectangle().fill(Tint.line).frame(height: 0.5)

                row(
                    title: "settings.language",
                    detail: "settings.languageHint"
                ) {
                    Picker("", selection: $prefs.language) {
                        ForEach(Prefs.Language.allCases) { Text($0.label).tag($0) }
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                    .fixedSize()
                    .tint(Tint.accent)
                }

                Rectangle().fill(Tint.line).frame(height: 0.5)

                // Without this the CLI is inside the app bundle and on nobody's
                // PATH, so `kw exec` — the command every agent instruction tells
                // people to run — is `command not found`.
                row(
                    title: "cli.title",
                    detail: cliError.map { LocalizedStringKey($0) }
                        ?? (cliState == .installed ? "cli.hintInstalled" : "cli.hintMissing")
                ) {
                    Button(cliState == .installed ? "cli.remove" : "cli.install") {
                        if cliState == .installed {
                            CLIInstall.uninstall()
                            cliError = nil
                        } else {
                            cliError = CLIInstall.install()
                        }
                        cliState = CLIInstall.state()
                    }
                    .buttonStyle(GhostButtonStyle())
                }

                Rectangle().fill(Tint.line).frame(height: 0.5)

                row(
                    title: "settings.launchAtLogin",
                    detail: loginError.map { LocalizedStringKey($0) } ?? "settings.launchAtLoginHint"
                ) {
                    Toggle(
                        "",
                        isOn: Binding(
                            get: { prefs.launchAtLogin },
                            set: { loginError = prefs.setLaunchAtLogin($0) }
                        )
                    )
                    .toggleStyle(.switch)
                    .labelsHidden()
                }
            }
            .background(Tint.surface)
            .clipShape(RoundedRectangle(cornerRadius: Radius.card))
            .overlay(
                RoundedRectangle(cornerRadius: Radius.card)
                    .strokeBorder(Tint.line, lineWidth: 0.5)
            )

            // The two renditions of the mark, side by side. Kept in the shipping
            // app rather than a style guide because it is the fastest way to spot
            // the small variant regressing (DESIGN.md §4).
            VStack(alignment: .leading, spacing: Space.s) {
                Text("settings.mark")
                    .font(Kind.label)
                    .foregroundStyle(Tint.ink2)
                HStack(alignment: .bottom, spacing: Space.xl) {
                    markSample(AnyView(AppMark(side: 56)), caption: "settings.mark.icon")
                    markSample(
                        AnyView(
                            WardMark(size: .large).fill(Tint.ink).frame(width: 16, height: 30)
                        ),
                        caption: "settings.mark.large"
                    )
                    markSample(
                        AnyView(
                            WardMark(size: .small).fill(Tint.ink).frame(width: 10, height: 18)
                        ),
                        caption: "settings.mark.small"
                    )
                }
            }
        }
        .padding(Space.xl)
        .frame(width: 460)
        .fixedSize(horizontal: false, vertical: true)
        .background(Tint.surface)
    }

    private func row<Control: View>(
        title: LocalizedStringKey,
        detail: LocalizedStringKey,
        @ViewBuilder control: () -> Control
    ) -> some View {
        HStack(alignment: .center, spacing: Space.m) {
            VStack(alignment: .leading, spacing: 1) {
                Text(title)
                    .font(Kind.rowTitle)
                    .tracking(Kind.rowTitleTracking)
                    .foregroundStyle(Tint.ink)
                Text(detail)
                    .font(Kind.meta)
                    .foregroundStyle(Tint.ink3)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: Space.m)
            control()
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 11)
        .background(Tint.surface)
    }

    private func markSample(_ content: AnyView, caption: LocalizedStringKey) -> some View {
        VStack(spacing: 7) {
            RoundedRectangle(cornerRadius: Radius.avatar)
                .fill(Tint.surface3)
                .frame(width: 68, height: 68)
                .overlay(content)
            Text(caption)
                .font(.system(size: 10, design: .monospaced))
                .foregroundStyle(Tint.ink3)
        }
    }
}
