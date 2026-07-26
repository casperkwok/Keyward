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

// `CLIInstall` used to live here: it symlinked the bundled `kw` into
// `/usr/local/bin` from a row in Settings that read "install this, or your
// terminal and coding agent cannot run kw exec".
//
// Both halves of that sentence stopped being true. The agent learns `kw`'s
// absolute path from the MCP tool descriptions and runs it from inside the
// bundle, and the person this is built for does not use a terminal at all. What
// was left was a button on the first screen a new user opens, telling them to
// install something before anything would work — for a product whose promise is
// that you add a key and say its name.

/// The loopback MCP endpoint, started beside the daemon.
///
/// Kept deliberately dumb: no toggle, no port setting, no lifecycle for anyone
/// to reason about. The one thing this exists to fix is the line a person pastes
/// into their coding agent — a URL that never changes instead of an absolute
/// path into the app bundle, which asked someone who has never opened a terminal
/// to think about where files live, and which broke whenever the app moved.
///
/// Always-on is safe here because the endpoint cannot answer a question about a
/// value. It serves names, `keyward://` references and a repository scan; any
/// process already running as this user can get the same three things from the
/// daemon socket directly.
@MainActor
enum MCPServer {
    static let port: UInt16 = 8787
    static var url: String { "http://127.0.0.1:\(port)/mcp" }

    /// Start it unless something already answers. Returns whether it is up.
    @discardableResult
    static func ensureRunning() async -> Bool {
        if isListening() { return true }
        guard let binary = DaemonLauncher.bundledCLI else { return false }

        let process = Process()
        process.executableURL = binary
        process.arguments = ["mcp", "--http", String(port)]
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        do {
            try process.run()
        } catch {
            return false
        }

        for _ in 0..<40 {
            try? await Task.sleep(for: .milliseconds(50))
            if isListening() { return true }
        }
        return false
    }

    /// A TCP connect, not an MCP handshake: this only has to answer "is the port
    /// taken by something", and a half-finished JSON-RPC exchange would be a
    /// slower way to learn the same thing.
    static func isListening() -> Bool {
        let fd = socket(AF_INET, SOCK_STREAM, 0)
        guard fd >= 0 else { return false }
        defer { close(fd) }
        var addr = sockaddr_in()
        addr.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        addr.sin_family = sa_family_t(AF_INET)
        addr.sin_port = port.bigEndian
        addr.sin_addr.s_addr = inet_addr("127.0.0.1")
        let ok = withUnsafePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                connect(fd, $0, socklen_t(MemoryLayout<sockaddr_in>.size)) == 0
            }
        }
        return ok
    }
}
