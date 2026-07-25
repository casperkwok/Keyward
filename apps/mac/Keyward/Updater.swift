//
//  Updater.swift
//  Sparkle, plus the one thing Sparkle cannot do for this app.
//
//  Sparkle replaces the whole bundle and relaunches. For most apps that is the
//  end of it. Keyward ships a *daemon inside the bundle* (§2), and that daemon is
//  a separate long-lived process: after an update the new app finds a daemon
//  already answering the socket and adopts it, because adopting a running daemon
//  is exactly what `DaemonLauncher` was written to do. The adopted daemon is the
//  old build, running from a path that no longer exists.
//
//  That is not a cosmetic mismatch. It is how a user ends up on a version whose
//  bug fixes are in the app they can see and not in the process that holds their
//  secrets — and, having watched an interrupted install leave an ad-hoc signed
//  daemon in place, it is a state that produces keychain prompts nobody can
//  explain.
//

import SwiftUI
import Sparkle

@MainActor
@Observable
final class Updater {
    static let shared = Updater()

    @ObservationIgnored let controller: SPUStandardUpdaterController
    var canCheck = true

    private init() {
        controller = SPUStandardUpdaterController(
            startingUpdater: true,
            updaterDelegate: nil,
            userDriverDelegate: nil
        )
        canCheck = controller.updater.canCheckForUpdates
        // Sparkle reports `canCheckForUpdates` as false until its own start-up
        // finishes, so re-read once it has — otherwise the menu item stays
        // disabled for the life of the process.
        Task { [weak self] in
            try? await Task.sleep(for: .seconds(1))
            self?.canCheck = self?.controller.updater.canCheckForUpdates ?? true
        }
    }

    func checkForUpdates() {
        controller.checkForUpdates(nil)
    }
}

/// Version agreement between the app and the daemon it talks to.
@MainActor
enum DaemonVersion {
    /// The app's own marketing version, which is what a release tags.
    static var app: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0"
    }

    /// Retire a daemon left behind by a previous version, then start ours.
    ///
    /// Deliberately does nothing when the versions agree: the common case is a
    /// daemon this same build started, and restarting it on every launch would
    /// drop every open broker session for no reason.
    ///
    /// Returns whether a daemon is reachable afterwards.
    @discardableResult
    static func reconcile() async -> Bool {
        guard let running = await DaemonClient.shared.versionIfRunning() else {
            return await DaemonLauncher.ensureRunning()
        }
        if running == app {
            return true
        }
        // Ask it to exit rather than killing it: the daemon holds a vault file and
        // an append-only log, and a SIGKILL between the write and the rename is
        // how a metadata file ends up truncated.
        await DaemonClient.shared.requestQuit()
        for _ in 0..<40 {
            try? await Task.sleep(for: .milliseconds(50))
            if await DaemonClient.shared.versionIfRunning() == nil { break }
        }
        return await DaemonLauncher.ensureRunning()
    }
}
