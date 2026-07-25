//
//  MenuBarController.swift
//  The status item, via AppKit rather than SwiftUI's `MenuBarExtra`.
//
//  `MenuBarExtra` produced no status item at all on macOS 26 here — not with a
//  custom image, not with an SF Symbol, not with the plain
//  `MenuBarExtra("…", systemImage:)` convenience form, and not as the only scene
//  in the app. Rather than keep bisecting an opaque SwiftUI failure, this drops to
//  `NSStatusItem`, which is what serious menu-bar apps use anyway: it gives
//  control over click handling, the popover's transient behaviour, and the
//  image's template rendering, none of which `MenuBarExtra` exposes.
//
//  Note on debugging this: the original symptom was "no icon appears", and it was
//  not caused by any of the above. On a notched MacBook the menu bar's right-hand
//  section runs from the notch to the clock, and once it is full macOS silently
//  drops new status items — no error, no log, nothing. Two unrelated menu-bar apps
//  vanishing at once is the tell. Check that before suspecting the drawing code.
//

import AppKit
import SwiftUI

@MainActor
final class MenuBarController: NSObject, NSPopoverDelegate {
    private var statusItem: NSStatusItem?
    private var popover: NSPopover?

    private let store: Store
    private let prefs: Prefs

    init(store: Store, prefs: Prefs) {
        self.store = store
        self.prefs = prefs
        super.init()
        install()
    }

    private func install() {
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        item.button?.image = Self.markImage
        item.button?.imagePosition = .imageOnly
        item.button?.target = self
        item.button?.action = #selector(toggle)
        item.button?.toolTip = "Keyward"
        statusItem = item

        let pop = NSPopover()
        pop.behavior = .transient
        pop.animates = true
        pop.delegate = self
        pop.contentViewController = NSHostingController(
            rootView: ContentView(compact: true)
                .environment(store)
                .environment(prefs)
                .tint(Tint.accent)
                .frame(width: 296, height: 400)
        )
        popover = pop
    }

    @objc private func toggle() {
        guard let popover, let button = statusItem?.button else { return }
        if popover.isShown {
            popover.performClose(nil)
        } else {
            popover.show(relativeTo: button.bounds, of: button, preferredEdge: .maxY)
            // Without this the popover opens behind whatever was frontmost.
            popover.contentViewController?.view.window?.makeKey()
        }
    }

    /// The mark at menu-bar size: the one-notch rendition, drawn from `WardMark`
    /// so it cannot drift from the shape the rest of the UI uses.
    ///
    /// `flipped: true` is load-bearing — AppKit's image context is y-up and every
    /// coordinate in `WardMark` is y-down, because that is what SwiftUI uses.
    private static let markImage: NSImage = {
        let image = NSImage(size: NSSize(width: 11, height: 18), flipped: true) { rect in
            NSColor.black.setFill()
            NSBezierPath(cgPath: WardMark(size: .small).path(in: rect).cgPath).fill()
            return true
        }
        // Template rendering lets the system tint it for light/dark menu bars and
        // invert it while the item is highlighted.
        image.isTemplate = true
        return image
    }()
}

/// Owns the status item for the process lifetime. A `MenuBarController` created
/// inside a SwiftUI `View` would be torn down with that view and take the icon
/// with it.
@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    var menuBar: MenuBarController?
    /// Held so closing the window hides it rather than destroying it.
    private var mainWindow: NSWindow?
    let store = Store()
    let prefs = Prefs()
    /// Runs for the app's lifetime, not a view's: a request can arrive while
    /// every window is closed, which is the common case — the user is in a
    /// terminal.
    let approvals = ApprovalWatcher()

    func applicationDidFinishLaunching(_ notification: Notification) {
        // `--snapshot <dir>` renders every screen off-screen and exits, so design
        // review never needs to drive the live UI.
        let args = CommandLine.arguments
        if let i = args.firstIndex(of: "--snapshot"), args.count > i + 1 {
            Snapshot.run(into: args[i + 1])
            NSApp.terminate(nil)
            return
        }
        Loc.use(localization: prefs.localization, locale: prefs.locale)
        // Before anything asks the socket a question. `ContentView.task` runs
        // `store.load()` immediately, and a failed first load leaves the user
        // looking at an error for a daemon that was about to start anyway.
        Task {
            // `reconcile`, not `ensureRunning`: after a Sparkle update the old
            // daemon is still answering, and adopting it would leave the process
            // that holds the secrets on the previous build (see `Updater.swift`).
            let up = await DaemonVersion.reconcile()
            if !up { store.reportDaemonUnavailable() }
            await store.load()
        }
        approvals.start()
        menuBar = MenuBarController(store: store, prefs: prefs)
        // The scene's window is created after this callback returns.
        DispatchQueue.main.async { [weak self] in self?.showMainWindow() }
    }

    /// Clicking the Dock icon with no window open reopens the main window.
    ///
    /// Returning `true` alone was not enough: a SwiftUI `Window` scene releases
    /// its `NSWindow` on close, so there was nothing left to bring forward and
    /// the click did nothing at all. `adoptMainWindow` keeps the window alive
    /// instead, which is also what a user means by closing one — put it away, not
    /// throw it out.
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows: Bool) -> Bool {
        showMainWindow()
        return true
    }

    func showMainWindow() {
        adoptMainWindow()
        guard let window = mainWindow else { return }
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    /// Take a strong reference to the window and stop AppKit releasing it.
    ///
    /// Called lazily rather than at launch because the scene's window does not
    /// exist yet when `applicationDidFinishLaunching` runs.
    private func adoptMainWindow() {
        if mainWindow != nil { return }
        let candidate = NSApp.windows.first { window in
            window.styleMask.contains(.titled) && !(window is NSPanel)
        }
        guard let candidate else { return }
        candidate.isReleasedWhenClosed = false
        mainWindow = candidate
    }
}
