//
//  DaemonLauncher.swift
//  Starts the bundled `keywardd` if nothing is answering the socket.
//
//  Until this existed the app was a viewer for a service nobody started. `kw`
//  told the user "open the Keyward app, then try again"; opening it did nothing,
//  so the instruction was false and every mechanism in the product was
//  unreachable to anyone who had not built it from source and run the daemon by
//  hand. That is the gap between "the parts work" and "the product works".
//

import AppKit
import Foundation

@MainActor
enum DaemonLauncher {
    /// Ensure a daemon is running, and return whether one is now reachable.
    ///
    /// Adopting a daemon that is already up matters as much as starting one: a
    /// developer running `cargo run -p keywardd` in a terminal must not have the
    /// app start a second copy fighting over the same socket.
    @discardableResult
    static func ensureRunning() async -> Bool {
        if await isAnswering() { return true }
        guard let binary = bundled else { return false }

        // A stale socket file from a crashed daemon makes bind() fail with
        // EADDRINUSE even though nothing is listening.
        let socket = URL(fileURLWithPath: DaemonClient.socketPath)
        if FileManager.default.fileExists(atPath: socket.path), !(await isAnswering()) {
            try? FileManager.default.removeItem(at: socket)
        }

        let process = Process()
        process.executableURL = binary
        // The daemon writes to stdout on start; nothing reads it, and an
        // unread pipe that fills would block the daemon rather than the app.
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        do {
            try process.run()
        } catch {
            return false
        }

        // Poll rather than sleep a fixed interval: binding a socket is fast, and
        // a fixed wait is either wasted time or too short on a loaded machine.
        for _ in 0..<40 {
            try? await Task.sleep(for: .milliseconds(50))
            if await isAnswering() { return true }
        }
        return false
    }

    /// `keywardd`, beside the app's own executable inside the bundle.
    static var bundled: URL? {
        guard let url = Bundle.main.executableURL?
            .deletingLastPathComponent()
            .appendingPathComponent("keywardd"),
            FileManager.default.isExecutableFile(atPath: url.path)
        else { return nil }
        return url
    }

    /// The bundled `kw`, for the PATH install in Settings.
    static var bundledCLI: URL? {
        guard let url = Bundle.main.executableURL?
            .deletingLastPathComponent()
            .appendingPathComponent("kw"),
            FileManager.default.isExecutableFile(atPath: url.path)
        else { return nil }
        return url
    }

    private static func isAnswering() async -> Bool {
        (try? await DaemonClient.shared.status()) != nil
    }
}

/// Where `kw` is linked from, and whether it is there.
///
/// `/usr/local/bin` rather than anywhere under the bundle: it is on the default
/// `PATH` of every shell on macOS, including the non-interactive ones a coding
/// agent spawns — which is the whole point, since the agent is the caller that
/// matters.
@MainActor
enum CLIInstall {
    static let destination = "/usr/local/bin/kw"

    enum State: Equatable {
        case installed
        case missing
        /// Something else owns the path — almost certainly another copy of the
        /// app, or a hand-built binary. Never overwrite it silently.
        case occupiedByOther(String)
    }

    static func state() -> State {
        let fm = FileManager.default
        guard let target = DaemonLauncher.bundledCLI else { return .missing }
        guard fm.fileExists(atPath: destination) else { return .missing }
        if let link = try? fm.destinationOfSymbolicLink(atPath: destination) {
            return link == target.path ? .installed : .occupiedByOther(link)
        }
        return .occupiedByOther(destination)
    }

    /// Returns an error message, or `nil` on success.
    ///
    /// Does not ask for administrator rights. If `/usr/local/bin` is not
    /// writable the honest move is to hand the user the one command that fixes
    /// it, rather than raising a password prompt from a tool whose entire pitch
    /// is that it does not need your credentials.
    static func install() -> String? {
        guard let target = DaemonLauncher.bundledCLI else {
            return Loc.t("cli.error.missing")
        }
        let fm = FileManager.default
        let directory = (destination as NSString).deletingLastPathComponent

        if !fm.fileExists(atPath: directory) {
            return Loc.t("cli.error.manual %@ %@", target.path, destination)
        }
        if case .occupiedByOther(let owner) = state() {
            return Loc.t("cli.error.occupied %@", owner)
        }
        do {
            if fm.fileExists(atPath: destination) {
                try fm.removeItem(atPath: destination)
            }
            try fm.createSymbolicLink(atPath: destination, withDestinationPath: target.path)
            return nil
        } catch {
            return Loc.t("cli.error.manual %@ %@", target.path, destination)
        }
    }

    static func uninstall() {
        if case .installed = state() {
            try? FileManager.default.removeItem(atPath: destination)
        }
    }
}
