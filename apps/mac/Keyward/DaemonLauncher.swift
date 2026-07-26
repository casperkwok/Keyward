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
    /// The port used unless it is taken. Remembered once chosen, so the address
    /// does not wander between launches.
    static let preferredPort: UInt16 = 8787
    private static let portKey = "mcpPort"

    private(set) static var port: UInt16 = UInt16(
        UserDefaults.standard.integer(forKey: portKey)
    ) == 0 ? preferredPort : UInt16(UserDefaults.standard.integer(forKey: portKey))

    static var url: String { "http://127.0.0.1:\(port)/mcp" }

    /// True whenever we are not on the usual address.
    ///
    /// Derived rather than remembered from the launch that moved us. A flag set
    /// once is false again the next time the app opens — and the stale entry in
    /// the agent's config does not heal itself in the meantime, so the warning
    /// has to survive a restart too.
    static var portChanged: Bool { port != preferredPort }

    /// Make sure *our* server is answering, and return whether it is.
    ///
    /// The check is an MCP `initialize` rather than a TCP connect, and that is
    /// the whole point of this function. A connect only proves the port is
    /// occupied; the first version treated "occupied" as "already running", so
    /// an unrelated program sitting on 8787 would have left Keyward not running
    /// while the agent happily talked to a stranger at the address we gave it.
    @discardableResult
    static func ensureRunning() async -> Bool {
        if identify(port) == .keyward { return true }

        // Ours is not there. Either nothing is, or somebody else is.
        if identify(port) == .someoneElse {
            guard let free = firstFreePort() else { return false }
            port = free
            UserDefaults.standard.set(Int(free), forKey: portKey)
        }

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
            if identify(port) == .keyward { return true }
        }
        return false
    }

    enum Occupant {
        case nobody
        case keyward
        case someoneElse
    }

    /// Who is on a port: nobody, us, or something else.
    ///
    /// "Us" means it answered an MCP `initialize` naming itself `keyward`. A
    /// weaker test — a successful connect, an HTTP 200 — would call any web
    /// server on that port a match.
    static func identify(_ port: UInt16) -> Occupant {
        guard let url = URL(string: "http://127.0.0.1:\(port)/mcp") else { return .nobody }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.timeoutInterval = 1.5
        request.setValue("application/json", forHTTPHeaderField: "content-type")
        request.httpBody = Data(
            #"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","clientInfo":{"name":"keyward-app","version":"1"}}}"#
                .utf8
        )

        // Synchronous on purpose: every caller is deciding what to do next and
        // has nothing to do in the meantime, and the timeout is 1.5s against
        // loopback.
        var occupant = Occupant.nobody
        let done = DispatchSemaphore(value: 0)
        URLSession.shared.dataTask(with: request) { data, response, _ in
            defer { done.signal() }
            guard let http = response as? HTTPURLResponse else { return }
            // Something answered HTTP, so the port is not free whatever it says.
            occupant = .someoneElse
            guard http.statusCode == 200, let data,
                let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                let result = json["result"] as? [String: Any],
                let info = result["serverInfo"] as? [String: Any],
                info["name"] as? String == "keyward"
            else { return }
            occupant = .keyward
        }.resume()
        _ = done.wait(timeout: .now() + 2.5)
        return occupant
    }

    /// The first port from the preferred one upwards with nothing on it.
    ///
    /// Deliberately a small window: if sixteen consecutive ports are taken, the
    /// machine has a problem that moving again will not solve.
    private static func firstFreePort() -> UInt16? {
        (preferredPort...(preferredPort + 15)).first { identify($0) == .nobody }
    }
}
