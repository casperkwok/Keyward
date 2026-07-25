//
//  Daemon.swift
//  The entire cross-language contract: newline-delimited JSON over a Unix socket
//  (ARCHITECTURE.md §7).
//
//  This app links no Rust and declares no bridging header. That is deliberate —
//  §2.1 — because a socket keeps the daemon a real security boundary rather than
//  a code-organisation convention, and because it lets this app be built against
//  `keywardd-stub` long before the real daemon exists.
//

import Foundation

// MARK: - Wire types

struct LastUse: Codable, Hashable {
    let at: Date
    let actor: String
    let project: String?
}

/// A broker session that is open right now. Carries no credential — the daemon
/// deliberately does not send one, so a view layer cannot log what it never had.
struct LiveSession: Codable, Hashable {
    let session: String
    let requests: Int
    let openedAt: Date
    let actor: String?

    enum CodingKeys: String, CodingKey {
        case session, requests, actor
        case openedAt = "opened_at"
    }
}

struct SecretSummary: Codable, Hashable, Identifiable {
    let name: String
    let display: String
    let ref: String
    let masked: String?
    let status: SecretStatus
    /// Avatar letter and tint. The daemon supplies them so the two desktop apps
    /// cannot drift into showing the same secret differently.
    let letter: String
    let tint: String
    /// Asset name of a bundled brand mark, when Keyward ships one for this
    /// service. `nil` falls back to the monogram — which is the common case, since
    /// a user may store a secret for anything.
    let logo: String?
    let lastUse: LastUse?
    /// Present while Keyward is forwarding for this secret.
    let live: LiveSession?

    var id: String { name }

    enum CodingKeys: String, CodingKey {
        case name, display, ref, masked, status, letter, tint, logo, live
        case lastUse = "last_use"
    }
}

struct UseRecord: Codable, Hashable, Identifiable {
    let at: Date
    let secret: String
    let actor: String
    let project: String?
    let tier: String
    let allowed: Bool

    var id: String { "\(at.timeIntervalSince1970)-\(actor)" }

    /// "Brokered" vs "handed over" — the only distinction the usage table draws.
    var wasBrokered: Bool { tier == "broker" || tier == "reference" }
}

/// The `{ok, result | error}` wrapper every response carries (§7).
private struct Envelope: Decodable {
    let ok: Bool
    let error: Failure?

    struct Failure: Decodable {
        let code: String
        let message: String
    }
}

private struct Payload<T: Decodable>: Decodable {
    let result: T
}

// MARK: - Errors

enum DaemonError: LocalizedError {
    case notRunning
    case connectionFailed(String)
    case protocolError(String)
    case refused(code: String, message: String)

    var errorDescription: String? {
        switch self {
        case .notRunning:
            String(localized: "error.notRunning")
        case .connectionFailed(let d):
            String(localized: "error.connectionFailed \(d)")
        case .protocolError(let d):
            String(localized: "error.protocolError \(d)")
        case .refused(_, let message):
            message
        }
    }
}

// MARK: - Client

/// One connection to `keywardd`, serialised through an actor.
///
/// Requests are issued one at a time and matched to the next line read back. The
/// protocol carries an `id` for pipelining, but nothing in this app benefits from
/// it — the main screen is a single `vault.list` — and a request/response map is
/// state that can go wrong for no gain.
actor DaemonClient {
    static let shared = DaemonClient()

    private var fd: Int32 = -1
    private var pending = Data()
    private var nextID = 1

    static var socketPath: String {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)
        let dir = base.first ?? URL(fileURLWithPath: NSHomeDirectory())
        return dir.appending(path: "Keyward/keywardd.sock").path(percentEncoded: false)
    }

    private var isConnected: Bool { fd >= 0 }

    // MARK: Public API

    func list() async throws -> [SecretSummary] {
        struct Reply: Decodable { let secrets: [SecretSummary] }
        return try await request("vault.list", Reply.self).secrets
    }

    func uses(of name: String, limit: Int = 50) async throws -> [UseRecord] {
        struct Reply: Decodable { let uses: [UseRecord] }
        return try await request(
            "uses.list",
            Reply.self,
            params: ["name": name, "limit": limit]
        ).uses
    }

    func add(name: String, display: String, value: String, upstream: String?) async throws {
        struct Reply: Decodable { let ok: Bool }
        var delivery: [String: Any] = ["kind": "handed"]
        if let upstream, !upstream.isEmpty {
            delivery = ["kind": "brokered", "upstream": upstream]
        }
        _ = try await request(
            "vault.add",
            Reply.self,
            params: ["name": name, "display": display, "value": value, "delivery": delivery]
        )
    }

    func rotate(name: String, value: String) async throws {
        struct Reply: Decodable { let ok: Bool }
        _ = try await request("vault.rotate", Reply.self, params: ["name": name, "value": value])
    }

    func setDisplay(name: String, display: String) async throws {
        struct Reply: Decodable { let ok: Bool }
        _ = try await request(
            "vault.set_display", Reply.self, params: ["name": name, "display": display]
        )
    }

    func remove(name: String) async throws {
        struct Reply: Decodable { let ok: Bool }
        _ = try await request("vault.remove", Reply.self, params: ["name": name])
    }

    /// Cheap liveness probe. Used by the launcher to tell "already running"
    /// from "needs starting", so a developer's hand-run daemon is adopted
    /// instead of fought with.
    func status() async throws -> Int {
        struct Reply: Decodable { let secrets: Int }
        return try await request("daemon.status", Reply.self).secrets
    }

    /// The running daemon's version, or `nil` if nothing is answering.
    ///
    /// Separate from `status()` because the caller that needs this — the
    /// post-update reconciliation — treats "not running" as an ordinary answer
    /// rather than an error.
    func versionIfRunning() async -> String? {
        struct Reply: Decodable { let version: String }
        return try? await request("daemon.status", Reply.self).version
    }

    /// Ask the daemon to shut down. Failure is not an error worth raising: if it
    /// has already gone, the caller wanted it gone.
    func requestQuit() async {
        struct Reply: Decodable { let quitting: Bool? }
        _ = try? await request("daemon.quit", Reply.self)
    }

    func pendingApprovals() async throws -> [PendingApproval] {
        struct Reply: Decodable { let pending: [PendingApproval] }
        return try await request("approval.pending", Reply.self).pending
    }

    func resolveApproval(id: String, decision: String) async throws {
        struct Reply: Decodable { let ok: Bool? }
        _ = try await request(
            "approval.resolve", Reply.self, params: ["id": id, "decision": decision]
        )
    }

    func closeSession(_ token: String) async throws {
        struct Reply: Decodable { let closed: Bool }
        _ = try await request("broker.close", Reply.self, params: ["session": token])
    }

    func disconnect() {
        if fd >= 0 { close(fd) }
        fd = -1
        pending.removeAll()
    }

    // MARK: Transport

    private func request<T: Decodable>(
        _ method: String,
        _ type: T.Type,
        params: [String: Any]? = nil
    ) async throws -> T {
        if !isConnected { try connect() }

        var body: [String: Any] = ["id": nextID, "method": method]
        nextID += 1
        if let params { body["params"] = params }

        var line = try JSONSerialization.data(withJSONObject: body)
        line.append(0x0A)

        do {
            try write(line)
            let response = try readLine()
            return try decode(response, as: type)
        } catch let error as DaemonError {
            // A half-open socket surfaces as a write or read failure. Drop it so
            // the next call reconnects rather than failing forever.
            if case .refused = error {} else { disconnect() }
            throw error
        }
    }

    private func decode<T: Decodable>(_ data: Data, as type: T.Type) throws -> T {
        let decoder = JSONDecoder()
        // Most timestamps on the wire are ISO 8601; `opened_at` is epoch seconds
        // because it comes from the broker's own clock rather than the log.
        decoder.dateDecodingStrategy = .custom { decoder in
            let container = try decoder.singleValueContainer()
            if let seconds = try? container.decode(Double.self) {
                return Date(timeIntervalSince1970: seconds)
            }
            let text = try container.decode(String.self)
            guard let date = ISO8601DateFormatter().date(from: text) else {
                throw DecodingError.dataCorruptedError(
                    in: container, debugDescription: "not a date: \(text)"
                )
            }
            return date
        }

        // Decode the envelope first: a refusal is a well-formed response, not a
        // parse failure, and must produce the daemon's message rather than
        // "unrecognised response".
        guard let envelope = try? decoder.decode(Envelope.self, from: data) else {
            throw DaemonError.protocolError("unrecognised response")
        }
        if !envelope.ok {
            throw DaemonError.refused(
                code: envelope.error?.code ?? "unknown",
                message: envelope.error?.message
                    ?? String(localized: "error.refusedNoReason")
            )
        }

        do {
            return try decoder.decode(Payload<T>.self, from: data).result
        } catch {
            throw DaemonError.protocolError(String(describing: error))
        }
    }

    private func connect() throws {
        let path = Self.socketPath
        guard FileManager.default.fileExists(atPath: path) else {
            throw DaemonError.notRunning
        }

        let handle = socket(AF_UNIX, SOCK_STREAM, 0)
        guard handle >= 0 else {
            throw DaemonError.connectionFailed(String(cString: strerror(errno)))
        }

        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        let bytes = Array(path.utf8)
        let capacity = MemoryLayout.size(ofValue: addr.sun_path)
        guard bytes.count < capacity else {
            close(handle)
            throw DaemonError.connectionFailed("socket path too long")
        }
        withUnsafeMutablePointer(to: &addr.sun_path) { raw in
            raw.withMemoryRebound(to: CChar.self, capacity: capacity) { dst in
                for (i, byte) in bytes.enumerated() { dst[i] = CChar(bitPattern: byte) }
                dst[bytes.count] = 0
            }
        }

        let size = socklen_t(MemoryLayout<sockaddr_un>.size)
        let result = withUnsafePointer(to: &addr) { pointer in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) { sa in
                Darwin.connect(handle, sa, size)
            }
        }
        guard result == 0 else {
            let reason = String(cString: strerror(errno))
            close(handle)
            throw DaemonError.connectionFailed(reason)
        }

        fd = handle
        pending.removeAll()
    }

    private func write(_ data: Data) throws {
        var sent = 0
        try data.withUnsafeBytes { raw in
            guard let base = raw.baseAddress else { return }
            while sent < data.count {
                let n = Darwin.write(fd, base.advanced(by: sent), data.count - sent)
                if n > 0 {
                    sent += n
                } else if errno == EINTR {
                    continue
                } else {
                    throw DaemonError.connectionFailed(String(cString: strerror(errno)))
                }
            }
        }
    }

    /// Read until the next newline, buffering whatever arrived past it.
    private func readLine() throws -> Data {
        while true {
            if let idx = pending.firstIndex(of: 0x0A) {
                let line = pending[pending.startIndex..<idx]
                pending = pending[pending.index(after: idx)...]
                return Data(line)
            }
            var chunk = [UInt8](repeating: 0, count: 8192)
            let n = read(fd, &chunk, chunk.count)
            if n > 0 {
                pending.append(contentsOf: chunk[0..<n])
            } else if n == 0 {
                throw DaemonError.connectionFailed("the daemon closed the connection")
            } else if errno == EINTR {
                continue
            } else {
                throw DaemonError.connectionFailed(String(cString: strerror(errno)))
            }
        }
    }
}
