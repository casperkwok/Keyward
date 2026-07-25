//
//  Snapshot.swift
//  Renders each screen to a PNG off-screen, for design review.
//
//  `Keyward --snapshot <dir>` writes one image per screen and exits. This exists
//  because the alternative — driving the live UI with synthetic mouse events to
//  take screenshots — hijacks the machine's input while someone is using it, and
//  because a screenshot of a real window also captures whatever the daemon
//  happened to return that second. These are deterministic.
//

import SwiftUI

@MainActor
enum Snapshot {
    /// Canned data matching the design mock, so the app and the mock can be
    /// compared directly rather than approximately.
    static func fixtures() -> ([SecretSummary], [UseRecord]) {
        func minutesAgo(_ m: Double) -> Date { Date(timeIntervalSinceNow: -m * 60) }

        let secrets = [
            SecretSummary(
                name: "stripe", display: "Stripe", ref: "keyward://stripe",
                masked: "sk_live_…4f2a", status: .protected, letter: "S", tint: "#5B51E8", logo: "StripeLogo",
                lastUse: LastUse(at: minutesAgo(2), actor: "codex", project: "my-shop"),
                live: LiveSession(
                    session: "kws_demo",
                    requests: 142,
                    openedAt: minutesAgo(12),
                    actor: "codex"
                )
            ),
            SecretSummary(
                name: "openai", display: "OpenAI", ref: "keyward://openai",
                masked: "sk-proj-…9c1b", status: .protected, letter: "O", tint: "#0E9C74", logo: "OpenAILogo",
                lastUse: LastUse(at: minutesAgo(12), actor: "claude", project: "my-shop"),
                live: nil
            ),
            SecretSummary(
                name: "pg-prod", display: "生产数据库", ref: "keyward://pg-prod",
                masked: "postgres://…", status: .shared, letter: "P", tint: "#2F6491", logo: "PostgresLogo",
                lastUse: LastUse(at: minutesAgo(64), actor: "psql", project: "my-shop"),
                live: nil
            ),
            SecretSummary(
                name: "resend", display: "Resend", ref: "keyward://resend",
                masked: "re_…7a2e", status: .unused, letter: "R", tint: "#DC4C2B", logo: "ResendLogo",
                lastUse: nil, live: nil
            ),
            SecretSummary(
                name: "github", display: "GitHub 令牌", ref: "keyward://github",
                masked: "ghp_…d13c", status: .protected, letter: "G", tint: "#7C51E8", logo: "GitHubLogo",
                lastUse: LastUse(at: minutesAgo(1_300), actor: "gh", project: "landing-page"),
                live: nil
            ),
        ]

        let uses: [UseRecord] = [
            (2.0, "codex", "broker"), (14, "codex", "broker"), (95, "claude", "broker"),
            (320, "npm run dev", "broker"), (1_300, "stripe-mcp", "broker"),
            (2_900, "codex", "broker"), (4_400, "psql", "injection"),
            (7_100, "codex", "broker"), (11_800, "claude", "broker"),
        ].map { minutes, actor, tier in
            UseRecord(
                at: minutesAgo(minutes), secret: "stripe", actor: actor,
                project: "my-shop", tier: tier, allowed: true
            )
        }
        return (secrets, uses)
    }

    /// A scan of a project that does not exist, produced by the real scanner over
    /// canned file contents.
    ///
    /// Hand-built `ImportFinding`s would render whatever the fixture claimed; this
    /// way a snapshot that looks wrong means the scanner is wrong, which is the
    /// only reason to render this screen off-screen at all. The values below are
    /// syntactically valid credentials that were never issued.
    static func importScan() -> ImportScan {
        let folder = URL(fileURLWithPath: NSHomeDirectory()).appending(path: "code/my-shop")

        let env = """
            # committed with the project
            PUBLIC_API_BASE=https://api.example.com
            ANTHROPIC_API_KEY=sk-ant-api03-Kf2mQ9xR7tLp0aWzVb4Nc8Ye
            DATABASE_URL=postgres://shop:s3cr3t-pw@db.internal:5432/shop
            """
        let envLocal = """
            STRIPE_SECRET_KEY=sk_live_51HxQ2mKa9PfVn3RtZbLw
            RESEND_API_KEY=re_8xKf2mQ9_xR7tLp0aWzVb
            """
        let mcp = """
            {
              "mcpServers": {
                "github": {
                  "command": "npx",
                  "args": ["-y", "@modelcontextprotocol/server-github"],
                  "env": { "GITHUB_TOKEN": "ghp_9xKf2mQ7tLp0aWzVb4Nc8YeR3dJ1" }
                }
              }
            }
            """

        let (mcpFindings, mcpCommands) = ImportScanner.scanMCP(mcp, within: nil)
        let files = ImportScanner.assignSlugs([
            ImportFile(
                url: folder.appending(path: ".env"), path: ".env", text: env,
                ignoredByGit: true, insideProject: true,
                findings: ImportScanner.scanDotenv(env), commands: []
            ),
            ImportFile(
                url: folder.appending(path: ".env.local"), path: ".env.local", text: envLocal,
                // Deliberately not ignored, so the warning and the `.gitignore`
                // option are in every snapshot rather than only in the happy case.
                ignoredByGit: false, insideProject: true,
                findings: ImportScanner.scanDotenv(envLocal), commands: []
            ),
            ImportFile(
                url: folder.appending(path: ".mcp.json"), path: ".mcp.json", text: mcp,
                ignoredByGit: true, insideProject: true,
                findings: mcpFindings, commands: mcpCommands
            ),
        ])

        return ImportScan(
            folder: folder,
            files: files,
            agent: .create(folder.appending(path: "AGENTS.md")),
            gitignore: folder.appending(path: ".gitignore")
        )
    }

    static func run(into directory: String) {
        let url = URL(fileURLWithPath: directory)
        try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)

        let (secrets, uses) = fixtures()

        for scheme in [ColorScheme.light, .dark] {
            for locale in ["zh-Hans", "en"] {
                let store = Store(secrets: secrets, uses: uses)
                let prefs = Prefs()
                let suffix = "\(scheme == .dark ? "dark" : "light")-\(locale)"

                write(
                    ContentView(),
                    store, prefs, scheme, locale,
                    size: CGSize(width: 620, height: 520),
                    to: url.appending(path: "list-\(suffix).png")
                )

                let detailStore = Store(secrets: secrets, uses: uses)
                detailStore.selected = secrets[0]
                write(
                    DetailView(secret: secrets[0]),
                    detailStore, prefs, scheme, locale,
                    size: CGSize(width: 620, height: 560),
                    to: url.appending(path: "detail-\(suffix).png")
                )

                write(
                    SettingsView(),
                    store, prefs, scheme, locale,
                    size: CGSize(width: 460, height: 420),
                    to: url.appending(path: "settings-\(suffix).png")
                )

                write(
                    ContentView(compact: true),
                    store, prefs, scheme, locale,
                    size: CGSize(width: 296, height: 400),
                    to: url.appending(path: "popover-\(suffix).png")
                )

                // The review state, which is the only one worth comparing: the
                // drop zone and the summary are a paragraph and a button, and the
                // diff is where a 40% longer Chinese string breaks a row.
                write(
                    ImportView(preview: importScan()),
                    store, prefs, scheme, locale,
                    size: CGSize(width: 640, height: 700),
                    to: url.appending(path: "import-\(suffix).png")
                )
            }
        }
        // Isolated avatar row, to tell "the list did not pass a logo" apart from
        // "the logo did not draw".
        write(
            HStack(spacing: 16) {
                Avatar(letter: "O", hex: "#0E9C74", logo: "OpenAILogo", side: 64)
                Avatar(letter: "D", hex: "#4D6BFE", logo: "DeepSeekLogo", side: 64)
                Avatar(letter: "S", hex: "#5B51E8", logo: nil, side: 64)
            }
            .padding(20),
            Store(secrets: [], uses: []), Prefs(), .light, "en",
            size: CGSize(width: 300, height: 104),
            to: url.appending(path: "zz-avatars.png")
        )
        print("snapshots written to \(directory)")
    }

    /// Renders through `NSHostingView` + `cacheDisplay`, not `ImageRenderer`.
    ///
    /// `ImageRenderer` draws SwiftUI's own primitives only: it renders AppKit-backed
    /// controls as a yellow "unavailable" bar and skips lazy containers entirely,
    /// which turned the first attempt at this into a screenshot of an empty list
    /// with a yellow stripe where the search field should be. `cacheDisplay` goes
    /// through the real AppKit drawing path, so what lands in the PNG is what the
    /// window would show.
    private static func write<V: View>(
        _ view: V,
        _ store: Store,
        _ prefs: Prefs,
        _ scheme: ColorScheme,
        _ locale: String,
        size: CGSize,
        to url: URL
    ) {
        Loc.use(localization: locale, locale: Locale(identifier: locale))
        let root =
            view
            .environment(store)
            .environment(prefs)
            .environment(\.locale, Locale(identifier: locale))
            .frame(width: size.width, height: size.height)

        let host = NSHostingView(rootView: AnyView(root))
        host.appearance = NSAppearance(named: scheme == .dark ? .darkAqua : .aqua)
        host.frame = CGRect(origin: .zero, size: size)
        host.layoutSubtreeIfNeeded()
        // One turn of the run loop so SwiftUI commits its first layout pass;
        // without it the hosting view renders before the content exists.
        RunLoop.current.run(until: Date().addingTimeInterval(0.35))

        guard let rep = host.bitmapImageRepForCachingDisplay(in: host.bounds) else {
            print("failed: \(url.lastPathComponent)")
            return
        }
        host.cacheDisplay(in: host.bounds, to: rep)
        guard let png = rep.representation(using: .png, properties: [:]) else {
            print("failed: \(url.lastPathComponent)")
            return
        }
        try? png.write(to: url)
    }
}
