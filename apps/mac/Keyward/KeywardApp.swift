//
//  KeywardApp.swift
//  The status item lives in `MenuBarController` (AppKit), not in a SwiftUI
//  `MenuBarExtra` — see the comment there for why.
//

import SwiftUI

@main
struct KeywardApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate

    var body: some Scene {
        // A real window as well as the status item. The shipping app is
        // menu-bar-first (DESIGN.md §1), but an app with no window is
        // undebuggable: when the status item fails to appear there is nothing
        // left to look at.
        Window("Keyward", id: "main") {
            ContentView()
                .environment(delegate.store)
                .environment(delegate.prefs)
                .preferredColorScheme(delegate.prefs.appearance.colorScheme)
                .environment(\.locale, delegate.prefs.locale)
                .onChange(of: delegate.prefs.locale) { Loc.use(localization: delegate.prefs.localization, locale: delegate.prefs.locale) }
                .tint(Tint.accent)
        }
        // A real toolbar, not a title bar with a strip of my own underneath it.
        //
        // The first attempt hid the title bar and drew a custom 52pt band. The
        // strip disappeared, but the traffic lights kept their system position at
        // the top of the window while the search field centred in the band — the
        // two were 10pt out and no height made both right. `unified(showsTitle:)`
        // hands the whole band to AppKit, which is the only thing that knows where
        // it puts the lights.
        .windowToolbarStyle(.unified(showsTitle: false))
        .defaultSize(width: 780, height: 560)
        .commands {
            // In the File menu beside `New`, because importing a project is the
            // other way a secret gets into Keyward. Routed through a notification
            // rather than shared state: `Commands` is built outside the scene's
            // view tree and cannot reach into `ContentView`'s `@State`, and giving
            // the sheet a home in `AppDelegate` would put window state in a type
            // that outlives every window.
            // Where macOS users look for it: directly under "About".
            CommandGroup(after: .appInfo) {
                Button("update.check") { Updater.shared.checkForUpdates() }
                    .disabled(!Updater.shared.canCheck)
            }
            CommandGroup(after: .newItem) {
                Button("import.menu") {
                    ContentView.openMainWindow()
                    NotificationCenter.default.post(name: .keywardImport, object: nil)
                }
                .keyboardShortcut("i", modifiers: [.command, .shift])
            }
        }
    }
}

extension Notification.Name {
    static let keywardImport = Notification.Name("ai.keyward.import")
}

extension NSBezierPath {
    /// SwiftUI draws with `CGPath`; `NSImage`'s drawing handler wants AppKit.
    convenience init(cgPath: CGPath) {
        self.init()
        cgPath.applyWithBlock { element in
            let points = element.pointee.points
            switch element.pointee.type {
            case .moveToPoint:
                move(to: points[0])
            case .addLineToPoint:
                line(to: points[0])
            case .addQuadCurveToPoint:
                // Elevate the quadratic to a cubic: AppKit has no quadratic form.
                let start = currentPoint
                let control = points[0]
                let end = points[1]
                curve(
                    to: end,
                    controlPoint1: CGPoint(
                        x: start.x + 2.0 / 3.0 * (control.x - start.x),
                        y: start.y + 2.0 / 3.0 * (control.y - start.y)
                    ),
                    controlPoint2: CGPoint(
                        x: end.x + 2.0 / 3.0 * (control.x - end.x),
                        y: end.y + 2.0 / 3.0 * (control.y - end.y)
                    )
                )
            case .addCurveToPoint:
                curve(to: points[2], controlPoint1: points[0], controlPoint2: points[1])
            case .closeSubpath:
                close()
            @unknown default:
                break
            }
        }
    }
}
