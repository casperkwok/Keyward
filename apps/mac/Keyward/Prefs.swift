//
//  Prefs.swift
//  Appearance and language, as product features rather than as whatever the OS
//  happens to be set to (DESIGN.md §8).
//

import ServiceManagement
import SwiftUI

@Observable
@MainActor
final class Prefs {
    enum Appearance: String, CaseIterable, Identifiable {
        case system, light, dark
        var id: String { rawValue }

        var label: LocalizedStringKey {
            switch self {
            case .system: "appearance.system"
            case .light: "appearance.light"
            case .dark: "appearance.dark"
            }
        }

        var colorScheme: ColorScheme? {
            switch self {
            case .system: nil
            case .light: .light
            case .dark: .dark
            }
        }
    }

    enum Language: String, CaseIterable, Identifiable {
        case system, en, zhHans = "zh-Hans"
        var id: String { rawValue }

        var label: String {
            switch self {
            case .system: Loc.t("language.system")
            case .en: "English"
            case .zhHans: "中文"
            }
        }
    }

    var appearance: Appearance {
        didSet { UserDefaults.standard.set(appearance.rawValue, forKey: "appearance") }
    }

    var language: Language {
        didSet { UserDefaults.standard.set(language.rawValue, forKey: "language") }
    }

    private(set) var launchAtLogin: Bool

    init() {
        let defaults = UserDefaults.standard
        appearance = Appearance(rawValue: defaults.string(forKey: "appearance") ?? "") ?? .system
        language = Language(rawValue: defaults.string(forKey: "language") ?? "") ?? .system
        launchAtLogin = SMAppService.mainApp.status == .enabled
    }

    /// The locale SwiftUI resolves `LocalizedStringKey` against.
    var locale: Locale {
        switch language {
        case .system: .autoupdatingCurrent
        case .en: Locale(identifier: "en")
        case .zhHans: Locale(identifier: "zh-Hans")
        }
    }

    /// Which `.lproj` ``Loc`` should read from.
    ///
    /// Derived from the same thing SwiftUI uses — the environment locale's
    /// *language* — and from nothing else. Two earlier attempts disagreed with
    /// `Text` and produced a sheet with English chrome around a Chinese body:
    ///
    /// - `locale.identifier` describes the **region**: on a Mac set to English in
    ///   China it is `zh_CN`.
    /// - `Bundle.preferredLocalizations` reads the AppleLanguages list, which can
    ///   also order differently from the resolved locale.
    ///
    /// `Text` resolves against `\.locale`, so this must too, or the two paths
    /// will keep drifting apart in ways that only show up on someone else's Mac.
    var localization: String {
        switch language {
        case .en: return "en"
        case .zhHans: return "zh-Hans"
        case .system:
            let code = locale.language.languageCode?.identifier ?? "en"
            return code == "zh" ? "zh-Hans" : code
        }
    }

    /// Returns an error message rather than throwing: a failed login item is a
    /// row that should stay switched off with an explanation, not an alert.
    func setLaunchAtLogin(_ on: Bool) -> String? {
        do {
            if on {
                try SMAppService.mainApp.register()
            } else {
                try SMAppService.mainApp.unregister()
            }
            launchAtLogin = SMAppService.mainApp.status == .enabled
            return nil
        } catch {
            launchAtLogin = SMAppService.mainApp.status == .enabled
            return error.localizedDescription
        }
    }
}

/// String lookup that follows the in-app language switch.
///
/// `String(localized:)` resolves against the *system* language and ignores
/// `\.environment(\.locale)`, so a language switch would move every `Text` in the
/// app while leaving strings built in Swift code stranded in the old language.
/// This routes those through the chosen language's bundle instead.
enum Loc {
    nonisolated(unsafe) static var bundle: Bundle = .main
    /// The locale in force. `RelativeDateTimeFormatter` and friends default to the
    /// *system* locale, so without this the timestamps stay English while every
    /// label around them switches.
    nonisolated(unsafe) static var locale: Locale = .autoupdatingCurrent

    /// `localization` is a language code from `Prefs.localization`; `locale`
    /// drives date formatting and may legitimately differ (English text, Chinese
    /// date conventions, on a Mac set that way).
    static func use(localization: String, locale: Locale) {
        Self.locale = locale
        guard
            let path = Bundle.main.path(forResource: localization, ofType: "lproj")
                ?? Bundle.main.path(forResource: String(localization.prefix(2)), ofType: "lproj"),
            let b = Bundle(path: path)
        else {
            bundle = .main
            return
        }
        bundle = b
    }

    static func t(_ key: String, _ args: CVarArg...) -> String {
        let format = bundle.localizedString(forKey: key, value: nil, table: nil)
        return args.isEmpty ? format : String(format: format, arguments: args)
    }
}

extension View {
    /// Carry the user's language choice into a view presented in its own window.
    ///
    /// Sheets, popovers and the approval window are separate windows on macOS and
    /// do not inherit `\.locale` from the scene that presented them, so a `Text`
    /// inside one resolves against the *system* language while a `Loc.t(...)`
    /// beside it resolves against the user's. The result is a dialog that is half
    /// translated, which is worse than either language on its own.
    func localised(_ prefs: Prefs) -> some View {
        environment(\.locale, prefs.locale)
    }
}
