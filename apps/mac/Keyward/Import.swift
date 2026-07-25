//
//  Import.swift
//  Scanning a project folder for literal secrets, and the plan for replacing them
//  with references (ARCHITECTURE.md §10.5).
//
//  All of this is deliberately free of SwiftUI: the screen that shows a diff of
//  someone's source files is only trustworthy if the thing computing that diff is
//  the same thing that later writes it. A view that re-derived the replacement at
//  write time could show one thing and do another.
//
//  Every edit is carried as a UTF-16 range into the file's original text and
//  applied back-to-front. Rewriting by re-serialising parsed JSON was the obvious
//  first design and it destroys the user's formatting and strips JSONC comments —
//  a diff nobody would approve. Range edits touch exactly the bytes shown.
//

import Foundation

// MARK: - What counts as a secret

enum SecretShape {
    /// Prefixes that are only ever seen on a live credential. Matched
    /// case-sensitively: `AKIA` is not `akia`, and lowering the bar here turns
    /// every `sk-` in a comment into a false positive the user has to think about.
    private static let prefixes = [
        "sk-", "sk_live_", "sk_test_", "rk_live_", "AKIA", "ASIA",
        "ghp_", "gho_", "ghu_", "ghs_", "github_pat_", "glpat-",
        "xoxb-", "xoxa-", "xoxp-", "xoxs-",
        "re_", "AIza", "dop_v1_", "hf_", "npm_", "SG.", "shpat_", "pplx-",
    ]

    /// A connection string is a secret only when it carries a password. The bare
    /// `postgres://localhost/dev` in a hundred READMEs is not one, and flagging it
    /// teaches the user to click through this screen without reading it.
    private static let urlWithPassword = try? NSRegularExpression(
        pattern: "^(postgres|postgresql|mysql|mongodb\\+srv|mongodb|redis|rediss|amqp)://[^:/@\\s]+:[^@\\s]+@"
    )

    static func isSecret(_ value: String) -> Bool {
        // A reference is the thing this whole screen produces. Flagging one would
        // offer to import an already-imported project and, worse, would let a
        // second run overwrite a good reference with a reference to a reference.
        if value.hasPrefix("keyward://") || value.hasPrefix("kw://") { return false }
        // Shell and CI interpolation is someone else's indirection, not a value.
        if value.hasPrefix("$") || value.contains("${") { return false }

        let range = NSRange(value.startIndex..., in: value)
        if urlWithPassword?.firstMatch(in: value, range: range) != nil { return true }

        // 12 is the shortest thing that is plausibly a live key: `re_` and `sk-`
        // are three characters and would otherwise match placeholder text like
        // `sk-xxx`.
        guard value.count >= 12 else { return false }
        return prefixes.contains { value.hasPrefix($0) }
    }

    /// Never the whole value, anywhere (DESIGN.md §7). The password inside a
    /// connection string is masked in place so the host and database — the parts
    /// that tell the user *which* database this is — survive.
    static func mask(_ value: String) -> String {
        if let regex = urlWithPassword,
            let match = regex.firstMatch(in: value, range: NSRange(value.startIndex..., in: value)),
            let whole = Range(match.range, in: value)
        {
            let head = value[whole]
            guard let colon = head.lastIndex(of: ":") else { return String(head) + "…" }
            return String(head[head.startIndex..<colon]) + ":…@" + String(value[whole.upperBound...])
        }
        guard value.count > 14 else { return String(value.prefix(4)) + "…" }
        return String(value.prefix(8)) + "…" + String(value.suffix(4))
    }
}

// MARK: - Naming

enum SecretNaming {
    /// Suffixes stripped to get from `STRIPE_SECRET_KEY` to `stripe`. Order
    /// matters — `AWS_ACCESS_KEY_ID` needs three passes.
    private static let suffixes = [
        "_ID", "_KEY", "_SECRET", "_TOKEN", "_URL", "_URI", "_API", "_ACCESS", "_DSN", "_PASSWORD",
    ]

    /// Names Keyward should spell the way the service does. Everything else gets
    /// title case, which is right far more often than it is wrong.
    private static let known = [
        "openai": "OpenAI", "anthropic": "Anthropic", "github": "GitHub",
        "gitlab": "GitLab", "aws": "AWS", "mysql": "MySQL", "mongodb": "MongoDB",
        "sendgrid": "SendGrid", "openrouter": "OpenRouter", "deepseek": "DeepSeek",
        "posthog": "PostHog", "npm": "npm", "huggingface": "Hugging Face",
    ]

    static func stem(from key: String) -> String {
        var name = key.uppercased()
        var changed = true
        while changed {
            changed = false
            for suffix in suffixes where name.hasSuffix(suffix) && name.count > suffix.count {
                name.removeLast(suffix.count)
                changed = true
            }
        }
        return name.isEmpty ? key.uppercased() : name
    }

    static func display(from key: String) -> String {
        let parts = stem(from: key).split(separator: "_").map(String.init)
        return parts.map { part -> String in
            let lower = part.lowercased()
            return known[lower] ?? lower.prefix(1).uppercased() + lower.dropFirst()
        }
        .joined(separator: " ")
    }
}

// MARK: - Model

/// One literal value found in one file, and where to put the reference.
struct ImportFinding: Identifiable, Hashable, Sendable {
    let id = UUID()
    /// The variable or JSON key the value sits under — what the user recognises.
    let key: String
    let value: String
    let masked: String
    let line: Int
    /// UTF-16 range of the value inside the file's original text.
    let range: NSRange
    /// The MCP server this belongs to, when the file is an MCP config.
    let server: String?
    var slug: String
    var display: String
    var include = true

    var ref: String { "keyward://\(slug)" }
}

/// Rewriting the launch line as well as the value (ARCHITECTURE.md §10.2). A
/// reference in an MCP config is inert without it: nothing resolves the ref, and
/// the server starts with the literal string as its credential.
struct CommandRewrite: Identifiable, Hashable, Sendable {
    let id = UUID()
    let server: String
    let line: Int
    /// What the diff shows. The edit itself spans from the command's value to the
    /// opening bracket of `args`, so the raw before/after are two halves of a JSON
    /// literal — true, and unreadable. These say the same thing in the shape the
    /// file has.
    let before: String
    let after: String
    /// What is actually written, over `range`.
    let replacement: String
    let range: NSRange
}

struct ImportFile: Identifiable, Sendable {
    let url: URL
    /// Shown in the diff header. Absolute for files outside the project, which is
    /// the whole point of showing it — `~/.claude.json` is not the user's project.
    let path: String
    let text: String
    /// Whether git would ignore this file. A `false` here has to reach the screen:
    /// rewriting a tracked file without saying so is the behaviour §10.5 calls
    /// "a tool people uninstall".
    let ignoredByGit: Bool
    /// `~/.claude.json` and friends live outside the folder and can never be
    /// covered by the project's `.gitignore`, so the warning would be noise.
    let insideProject: Bool
    var findings: [ImportFinding]
    var commands: [CommandRewrite]

    var id: URL { url }

    var included: [ImportFinding] { findings.filter(\.include) }

    /// The rewritten text, with only the included findings applied.
    ///
    /// Applied back-to-front so that each replacement leaves the offsets of the
    /// edits before it untouched; the alternative is recomputing every range after
    /// every edit, which is the same algorithm with more places to be wrong.
    func rewritten() -> String {
        let servers = Set(included.compactMap(\.server))
        var edits: [(NSRange, String)] = included.map { ($0.range, $0.ref) }
        edits += commands
            .filter { servers.contains($0.server) }
            .map { ($0.range, $0.replacement) }

        let out = NSMutableString(string: text)
        for (range, replacement) in edits.sorted(by: { $0.0.location > $1.0.location }) {
            out.replaceCharacters(in: range, with: replacement)
        }
        return out as String
    }
}

/// Where the agent instruction block goes.
enum AgentTarget: Sendable {
    case existing(URL)
    case create(URL)

    var url: URL {
        switch self {
        case .existing(let url), .create(let url): url
        }
    }

    var isNew: Bool {
        if case .create = self { return true }
        return false
    }
}

struct ImportScan: Sendable {
    let folder: URL
    var files: [ImportFile]
    let agent: AgentTarget
    /// `.gitignore` exists and already covers every file being touched.
    let gitignore: URL

    var findings: [ImportFinding] { files.flatMap(\.findings) }
    var includedCount: Int { files.reduce(0) { $0 + $1.included.count } }
    var isEmpty: Bool { findings.isEmpty }

    /// Files that git would commit as they stand. Drives the warning, and the
    /// default state of the "add to .gitignore" option.
    var exposedFiles: [ImportFile] {
        files.filter { !$0.ignoredByGit && $0.insideProject && !$0.included.isEmpty }
    }
}

/// What an import did, in enough detail to undo it.
struct ImportReceipt: Sendable {
    let folder: URL
    let backups: URL
    /// Files whose originals are in `backups`, keyed by the name used there.
    var restorable: [(url: URL, backup: URL)] = []
    /// Files Keyward created. Undo deletes these rather than restoring them.
    var created: [URL] = []
    var addedSecrets: [String] = []
    var rewrittenCount = 0
}

// MARK: - .gitignore

/// Enough of git's ignore rules to answer one question honestly: would git commit
/// this file?
///
/// Deliberately not a full implementation — no `**`, no nested `.gitignore`, no
/// index awareness. It errs towards reporting a file as *not* ignored, so the
/// failure mode is an extra warning rather than a silent rewrite of a tracked file.
struct GitIgnore: Sendable {
    private struct Rule: Sendable {
        let pattern: String
        let negated: Bool
        let anchored: Bool
        let directoryOnly: Bool
    }

    private let rules: [Rule]

    init(folder: URL) {
        let text = (try? String(contentsOf: folder.appending(path: ".gitignore"), encoding: .utf8)) ?? ""
        rules = text.split(separator: "\n", omittingEmptySubsequences: false).compactMap { raw in
            var line = raw.trimmingCharacters(in: .whitespaces)
            guard !line.isEmpty, !line.hasPrefix("#") else { return nil }
            let negated = line.hasPrefix("!")
            if negated { line.removeFirst() }
            let directoryOnly = line.hasSuffix("/")
            if directoryOnly { line.removeLast() }
            let anchored = line.contains("/")
            if line.hasPrefix("/") { line.removeFirst() }
            guard !line.isEmpty else { return nil }
            return Rule(
                pattern: line, negated: negated, anchored: anchored, directoryOnly: directoryOnly
            )
        }
    }

    /// Last matching rule wins, which is what git does and why `!` works at all.
    func ignores(_ relativePath: String) -> Bool {
        var ignored = false
        let basename = (relativePath as NSString).lastPathComponent
        for rule in rules {
            // A directory rule covers what is beneath it and nothing else, so
            // `secrets/` covers `secrets/.env` but never a file called `secrets`.
            let matched: Bool
            if rule.directoryOnly {
                matched = relativePath.hasPrefix(rule.pattern + "/")
            } else {
                let subject = rule.anchored ? relativePath : basename
                matched =
                    fnmatch(rule.pattern, subject, rule.anchored ? FNM_PATHNAME : 0) == 0
                    || relativePath.hasPrefix(rule.pattern + "/")
            }
            if matched { ignored = !rule.negated }
        }
        return ignored
    }
}

// MARK: - Scanning

enum ImportScanner {
    /// Runs on whatever actor calls it, on purpose.
    ///
    /// The whole scan is a handful of files under a megabyte; moving it to a
    /// background executor would buy a millisecond and cost the caller a set of
    /// `Sendable` hoops around a type that is only ever read on the main actor.
    static func scan(folder: URL) -> ImportScan {
        let fm = FileManager.default
        let ignore = GitIgnore(folder: folder)
        var files: [ImportFile] = []

        let names = (try? fm.contentsOfDirectory(atPath: folder.path(percentEncoded: false))) ?? []
        let dotenvs = names
            .filter { $0 == ".env" || $0.hasPrefix(".env.") }
            .sorted()
        let mcps = names.filter { $0 == "mcp.json" || $0 == ".mcp.json" }.sorted()

        for name in dotenvs {
            let url = folder.appending(path: name)
            guard let text = try? String(contentsOf: url, encoding: .utf8) else { continue }
            let findings = scanDotenv(text)
            guard !findings.isEmpty else { continue }
            files.append(
                ImportFile(
                    url: url, path: name, text: text,
                    ignoredByGit: ignore.ignores(name), insideProject: true,
                    findings: findings, commands: []
                )
            )
        }

        for name in mcps {
            let url = folder.appending(path: name)
            guard let text = try? String(contentsOf: url, encoding: .utf8) else { continue }
            let (findings, commands) = scanMCP(text, within: nil)
            guard !findings.isEmpty else { continue }
            files.append(
                ImportFile(
                    url: url, path: name, text: text,
                    ignoredByGit: ignore.ignores(name), insideProject: true,
                    findings: findings, commands: commands
                )
            )
        }

        // `~/.claude.json` holds every project the user has ever opened. Only the
        // block for *this* folder is in scope — rewriting another project's
        // credentials from a screen titled with this project's name would be a
        // surprise of exactly the kind §10.5 warns about.
        let claude = URL(fileURLWithPath: NSHomeDirectory()).appending(path: ".claude.json")
        if let text = try? String(contentsOf: claude, encoding: .utf8),
            let scope = projectScope(in: text, path: folder.path(percentEncoded: false))
        {
            let (findings, commands) = scanMCP(text, within: scope)
            if !findings.isEmpty {
                files.append(
                    ImportFile(
                        url: claude, path: "~/.claude.json", text: text,
                        ignoredByGit: true, insideProject: false,
                        findings: findings, commands: commands
                    )
                )
            }
        }

        return ImportScan(
            folder: folder,
            files: assignSlugs(files),
            agent: agentTarget(in: folder),
            gitignore: folder.appending(path: ".gitignore")
        )
    }

    static func agentTarget(in folder: URL) -> AgentTarget {
        let fm = FileManager.default
        for name in ["CLAUDE.md", "AGENTS.md"] {
            let url = folder.appending(path: name)
            if fm.fileExists(atPath: url.path(percentEncoded: false)) { return .existing(url) }
        }
        return .create(folder.appending(path: "AGENTS.md"))
    }

    /// One slug per distinct value, not per occurrence.
    ///
    /// The same key is routinely in `.env` and in an MCP config. Importing it
    /// twice would put two copies of one credential in the keychain and rotate
    /// only one of them later.
    static func assignSlugs(_ files: [ImportFile]) -> [ImportFile] {
        var byValue: [String: (slug: String, display: String)] = [:]
        var taken = Set<String>()

        return files.map { file in
            var file = file
            file.findings = file.findings.map { finding in
                var finding = finding
                if let existing = byValue[finding.value] {
                    finding.slug = existing.slug
                    finding.display = existing.display
                    return finding
                }
                let display = SecretNaming.display(from: finding.key)
                var slug = Slug.make(from: display) ?? "secret"
                var n = 2
                while taken.contains(slug) {
                    slug = "\(Slug.make(from: display) ?? "secret")-\(n)"
                    n += 1
                }
                taken.insert(slug)
                byValue[finding.value] = (slug, display)
                finding.slug = slug
                finding.display = display
                return finding
            }
            return file
        }
    }

    // MARK: dotenv

    static func scanDotenv(_ text: String) -> [ImportFinding] {
        var findings: [ImportFinding] = []
        for (index, line) in text.split(separator: "\n", omittingEmptySubsequences: false).enumerated() {
            let trimmed = line.drop { $0 == " " || $0 == "\t" }
            guard !trimmed.isEmpty, !trimmed.hasPrefix("#") else { continue }
            let body = trimmed.hasPrefix("export ") ? trimmed.dropFirst(7) : trimmed
            guard let equals = body.firstIndex(of: "=") else { continue }

            let key = body[body.startIndex..<equals].trimmingCharacters(in: .whitespaces)
            guard !key.isEmpty else { continue }

            var start = body.index(after: equals)
            var end = body.endIndex
            // Trailing comment, but only on an unquoted value — ` #` inside quotes
            // is part of a password often enough to matter.
            let raw = body[start..<end]
            let quote = raw.first
            if quote == "\"" || quote == "'" {
                guard let closing = raw.dropFirst().firstIndex(of: quote!) else { continue }
                start = raw.index(after: raw.startIndex)
                end = closing
            } else {
                if let hash = raw.range(of: " #") { end = hash.lowerBound }
                while end > start, body[body.index(before: end)] == " " || body[body.index(before: end)] == "\r" {
                    end = body.index(before: end)
                }
            }
            guard start < end else { continue }

            let value = String(body[start..<end])
            guard SecretShape.isSecret(value) else { continue }
            findings.append(
                ImportFinding(
                    key: key, value: value, masked: SecretShape.mask(value),
                    line: index + 1, range: NSRange(start..<end, in: text),
                    server: nil, slug: "", display: ""
                )
            )
        }
        return findings
    }

    // MARK: JSON

    private static let pairPattern = try? NSRegularExpression(
        pattern: "\"([A-Za-z_][A-Za-z0-9_]*)\"\\s*:\\s*\"((?:[^\"\\\\]|\\\\.)*)\""
    )
    private static let commandPattern = try? NSRegularExpression(
        pattern: "\"command\"\\s*:\\s*\"((?:[^\"\\\\]|\\\\.)*)\""
    )
    private static let argsPattern = try? NSRegularExpression(pattern: "\"args\"\\s*:\\s*\\[")

    /// Scans MCP server definitions, optionally restricted to a sub-range of the
    /// file (the project's own block inside `~/.claude.json`).
    ///
    /// Textual rather than `JSONSerialization` because these files are routinely
    /// JSONC — the `mcp.json` in ARCHITECTURE.md §10.2 has a trailing comment on
    /// the very line that matters — and a parser that refuses the file would
    /// silently report "nothing found" for a project full of live keys.
    static func scanMCP(_ text: String, within scope: NSRange?) -> ([ImportFinding], [CommandRewrite]) {
        let whole = scope ?? NSRange(text.startIndex..., in: text)
        guard let servers = objectValue(of: "mcpServers", in: text, within: whole) else {
            return ([], [])
        }

        var findings: [ImportFinding] = []
        var commands: [CommandRewrite] = []

        for (name, body) in directKeys(in: text, within: servers) {
            guard let env = objectValue(of: "env", in: text, within: body) else { continue }
            var found = false
            for match in pairPattern?.matches(in: text, range: env) ?? [] {
                guard let keyRange = Range(match.range(at: 1), in: text),
                    let valueRange = Range(match.range(at: 2), in: text)
                else { continue }
                let value = unescape(String(text[valueRange]))
                guard SecretShape.isSecret(value) else { continue }
                found = true
                findings.append(
                    ImportFinding(
                        key: String(text[keyRange]), value: value,
                        masked: SecretShape.mask(value),
                        line: lineNumber(of: match.range(at: 2).location, in: text),
                        range: match.range(at: 2), server: name, slug: "", display: ""
                    )
                )
            }
            guard found else { continue }
            if let rewrite = commandRewrite(in: text, within: body, server: name) {
                commands.append(rewrite)
            }
        }
        return (findings, commands)
    }

    /// `"command": "npx", "args": [...]` → `"command": "kw", "args": ["exec", "--", "npx", …]`.
    ///
    /// Only when both keys are present. Rewriting `command` alone would leave a
    /// server that launches `kw` with the wrong arguments — a broken project in
    /// exchange for a protection that never engages.
    private static func commandRewrite(in text: String, within body: NSRange, server: String) -> CommandRewrite? {
        guard let command = commandPattern?.firstMatch(in: text, range: body),
            let args = argsPattern?.firstMatch(in: text, range: body),
            let commandValue = Range(command.range(at: 1), in: text)
        else { return nil }
        let program = String(text[commandValue])
        guard program != "kw" else { return nil }

        // One edit spanning from the command's value to just past the `[`, so the
        // two halves cannot be applied independently and leave a half-rewritten
        // launch line.
        let start = command.range(at: 1).location
        let end = args.range.location + args.range.length
        guard end > start else { return nil }
        let span = NSRange(location: start, length: end - start)
        guard let spanRange = Range(span, in: text) else { return nil }

        let raw = String(text[spanRange])
        let replacement =
            raw.replacingOccurrences(
                of: program, with: "kw", options: [], range: raw.range(of: program)
            ) + "\"exec\", \"--\", \"\(program)\", "

        return CommandRewrite(
            server: server,
            line: lineNumber(of: start, in: text),
            before: "\"command\": \"\(program)\"",
            after: "\"command\": \"kw\", \"args\": [\"exec\", \"--\", \"\(program)\", …]",
            replacement: replacement,
            range: span
        )
    }

    /// The `projects["/path/to/folder"]` block, if this project has one — the
    /// "if the project references it" clause in the brief. Absent means the user
    /// has never opened this folder in Claude Code, so there is nothing of theirs
    /// in that file to rewrite.
    static func projectScope(in text: String, path: String) -> NSRange? {
        guard let projects = objectValue(of: "projects", in: text, within: NSRange(text.startIndex..., in: text))
        else { return nil }
        // Trailing-slash forms of the same path are the same project.
        let wanted = path.hasSuffix("/") ? String(path.dropLast()) : path
        for (name, body) in directKeys(in: text, within: projects) {
            let candidate = name.hasSuffix("/") ? String(name.dropLast()) : name
            if candidate == wanted { return body }
        }
        return nil
    }

    // MARK: JSON text helpers

    /// The `{…}` value of `key`, searched at any depth inside `range`.
    private static func objectValue(of key: String, in text: String, within range: NSRange) -> NSRange? {
        let ns = text as NSString
        var search = range
        while search.length > 0 {
            let found = ns.range(of: "\"\(key)\"", options: [], range: search)
            guard found.location != NSNotFound else { return nil }
            if let body = braceRange(in: ns, from: found.location + found.length, limit: range) {
                return body
            }
            let next = found.location + found.length
            search = NSRange(location: next, length: max(0, range.location + range.length - next))
        }
        return nil
    }

    /// Every `"name": { … }` one level inside `object`, in file order.
    private static func directKeys(in text: String, within object: NSRange) -> [(String, NSRange)] {
        let ns = text as NSString
        var result: [(String, NSRange)] = []
        var i = object.location
        let end = object.location + object.length
        var depth = 0
        var pendingKey: String?

        while i < end {
            let ch = ns.character(at: i)
            switch ch {
            case UInt16(UInt8(ascii: "\"")) where depth == 1:
                guard let (literal, next) = string(in: ns, at: i, limit: end) else { return result }
                pendingKey = literal
                i = next
                continue
            case UInt16(UInt8(ascii: "\"")):
                guard let (_, next) = string(in: ns, at: i, limit: end) else { return result }
                i = next
                continue
            case UInt16(UInt8(ascii: "{")), UInt16(UInt8(ascii: "[")):
                depth += 1
                if depth == 2, ch == UInt16(UInt8(ascii: "{")), let key = pendingKey,
                    let body = braceRange(in: ns, from: i, limit: object)
                {
                    result.append((key, body))
                    i = body.location + body.length
                    depth -= 1
                    pendingKey = nil
                    continue
                }
            case UInt16(UInt8(ascii: "}")), UInt16(UInt8(ascii: "]")):
                depth -= 1
                if depth == 0 { return result }
            default:
                break
            }
            i += 1
        }
        return result
    }

    /// The `{ … }` starting at or after `from`, balanced, ignoring braces inside
    /// strings. Returns the range including both braces.
    private static func braceRange(in ns: NSString, from: Int, limit: NSRange) -> NSRange? {
        let end = limit.location + limit.length
        var i = from
        while i < end, ns.character(at: i) != UInt16(UInt8(ascii: "{")) {
            let ch = ns.character(at: i)
            // Anything other than whitespace or the colon means this key's value
            // is not an object.
            if ch != UInt16(UInt8(ascii: ":")), ch != 0x20, ch != 0x09, ch != 0x0A, ch != 0x0D {
                return nil
            }
            i += 1
        }
        guard i < end else { return nil }

        let start = i
        var depth = 0
        while i < end {
            let ch = ns.character(at: i)
            if ch == UInt16(UInt8(ascii: "\"")) {
                guard let (_, next) = string(in: ns, at: i, limit: end) else { return nil }
                i = next
                continue
            }
            if ch == UInt16(UInt8(ascii: "{")) { depth += 1 }
            if ch == UInt16(UInt8(ascii: "}")) {
                depth -= 1
                if depth == 0 { return NSRange(location: start, length: i - start + 1) }
            }
            i += 1
        }
        return nil
    }

    /// The JSON string literal starting at `at`, and the index just past it.
    private static func string(in ns: NSString, at: Int, limit: Int) -> (String, Int)? {
        var i = at + 1
        var out = ""
        while i < limit {
            let ch = ns.character(at: i)
            if ch == UInt16(UInt8(ascii: "\\")) {
                i += 2
                continue
            }
            if ch == UInt16(UInt8(ascii: "\"")) {
                out = ns.substring(with: NSRange(location: at + 1, length: i - at - 1))
                return (unescape(out), i + 1)
            }
            i += 1
        }
        return nil
    }

    private static func unescape(_ s: String) -> String {
        s.replacingOccurrences(of: "\\\"", with: "\"")
            .replacingOccurrences(of: "\\/", with: "/")
            .replacingOccurrences(of: "\\\\", with: "\\")
    }

    private static func lineNumber(of location: Int, in text: String) -> Int {
        let ns = text as NSString
        guard location <= ns.length else { return 1 }
        let head = ns.substring(to: location)
        return head.reduce(1) { $1 == "\n" ? $0 + 1 : $0 }
    }
}

// MARK: - Writing

enum ImportWriter {
    enum Failure: LocalizedError {
        case backup(String)
        case write(String)

        var errorDescription: String? {
            switch self {
            case .backup(let d): Loc.t("import.error.backup %@", d)
            case .write(let d): Loc.t("import.error.write %@", d)
            }
        }
    }

    /// Copies the original somewhere Keyward owns, before anything is written.
    ///
    /// Backups go to Application Support rather than a sibling `.bak` in the
    /// project: a `.env.bak` full of live keys, next to a `.env` that no longer
    /// has any, is the import undoing its own point. Nothing is written to the
    /// project until every backup has succeeded.
    static func backUp(_ files: [URL], into receipt: inout ImportReceipt) throws {
        let fm = FileManager.default
        try fm.createDirectory(at: receipt.backups, withIntermediateDirectories: true)
        for url in files {
            let name = url.path(percentEncoded: false)
                .replacingOccurrences(of: "/", with: "%")
            let destination = receipt.backups.appending(path: name)
            do {
                try fm.copyItem(at: url, to: destination)
            } catch {
                throw Failure.backup(error.localizedDescription)
            }
            receipt.restorable.append((url: url, backup: destination))
        }
    }

    static func write(_ text: String, to url: URL) throws {
        do {
            try text.write(to: url, atomically: true, encoding: .utf8)
        } catch {
            throw Failure.write(error.localizedDescription)
        }
    }

    /// Appends the agent block, or creates the file holding it.
    ///
    /// Skipped when the block is already there, so re-importing a project does not
    /// stack four copies of the same instructions into `CLAUDE.md` — which is how
    /// an agent ends up reading a contradictory list of rules.
    static func writeAgentBlock(_ block: String, to target: AgentTarget, receipt: inout ImportReceipt) throws {
        let url = target.url
        let existing = (try? String(contentsOf: url, encoding: .utf8)) ?? ""
        guard !existing.contains(Loc.t("agent.heading")) else { return }

        if target.isNew {
            receipt.created.append(url)
            try write(block + "\n", to: url)
            return
        }
        let separator = existing.hasSuffix("\n") ? "\n" : "\n\n"
        try write(existing + separator + block + "\n", to: url)
    }

    /// Adds the rewritten files to `.gitignore`.
    ///
    /// The rewritten `.env` holds only references and is safe to commit
    /// (ARCHITECTURE.md §8.1), so this is not what makes the import safe — the
    /// point is that the *next* person to paste a key into that file is covered.
    static func addToGitignore(_ paths: [String], at url: URL, receipt: inout ImportReceipt) throws {
        guard !paths.isEmpty else { return }
        let existing = (try? String(contentsOf: url, encoding: .utf8)) ?? ""
        if !FileManager.default.fileExists(atPath: url.path(percentEncoded: false)) {
            receipt.created.append(url)
        }
        let separator = existing.isEmpty || existing.hasSuffix("\n") ? "" : "\n"
        let block = "\n# Keyward\n" + paths.joined(separator: "\n") + "\n"
        try write(existing + separator + block, to: url)
    }

    /// Puts every file back and throws the created ones away.
    ///
    /// Errors are collected rather than thrown: a half-completed undo that stops
    /// at the first failure leaves the project in a state neither the user nor
    /// Keyward can describe. Restore what can be restored, then say what could not.
    static func undo(_ receipt: ImportReceipt) -> [String] {
        let fm = FileManager.default
        var problems: [String] = []

        for entry in receipt.restorable {
            do {
                let text = try String(contentsOf: entry.backup, encoding: .utf8)
                try text.write(to: entry.url, atomically: true, encoding: .utf8)
            } catch {
                problems.append("\(entry.url.lastPathComponent): \(error.localizedDescription)")
            }
        }
        for url in receipt.created where !receipt.restorable.contains(where: { $0.url == url }) {
            try? fm.removeItem(at: url)
        }
        try? fm.removeItem(at: receipt.backups)
        return problems
    }

    static func backupDirectory() -> URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)
        let root = base.first ?? URL(fileURLWithPath: NSHomeDirectory())
        let stamp = ISO8601DateFormatter().string(from: .now).replacingOccurrences(of: ":", with: "-")
        return root.appending(path: "Keyward/imports/\(stamp)")
    }
}
