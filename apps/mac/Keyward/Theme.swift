//
//  Theme.swift
//  The DESIGN.md token system, transcribed. Hex values here must match §2 exactly;
//  when they diverge, DESIGN.md is the source of truth and this file is the bug.
//

import SwiftUI

extension Color {
    /// A colour that resolves per appearance, so views never branch on
    /// `@Environment(\.colorScheme)` to pick a fill. Dark mode is not an
    /// inversion (DESIGN.md §6) — each token carries its own dark value.
    init(light: String, dark: String) {
        self.init(nsColor: NSColor(name: nil) { appearance in
            let isDark = appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
            return NSColor(hex: isDark ? dark : light)
        })
    }
}

extension NSColor {
    fileprivate convenience init(hex: String) {
        let s = hex.hasPrefix("#") ? String(hex.dropFirst()) : hex
        let v = UInt32(s, radix: 16) ?? 0
        self.init(
            srgbRed: CGFloat((v >> 16) & 0xFF) / 255,
            green: CGFloat((v >> 8) & 0xFF) / 255,
            blue: CGFloat(v & 0xFF) / 255,
            alpha: 1
        )
    }
}

/// DESIGN.md §2 — Color Palette & Roles.
enum Tint {
    // The accent is neutral: graphite in light, near-white in dark. An earlier
    // draft made it jade on the theory that "the accent *is* the protected
    // colour" — which broke §1's own rule that colour appears only where it
    // carries state, and left the app with a green button, a green label and a
    // green status dot competing for the same meaning.
    //
    // With a neutral accent the two status colours are the only colour in the
    // product, which is what the rule was for.
    static let accent = Color(light: "#1D1F22", dark: "#EDEEEF")
    static let accentTop = Color(light: "#35393E", dark: "#FBFBFB")
    static let accentBottom = Color(light: "#1B1E21", dark: "#DFE1E2")
    /// Text and glyphs drawn on top of `accentFill`.
    static let onAccent = Color(light: "#FFFFFF", dark: "#16181A")
    static let accentWash = Color(light: "#ECEDEE", dark: "#2A2D30")

    static var accentFill: LinearGradient {
        LinearGradient(colors: [accentTop, accentBottom], startPoint: .top, endPoint: .bottom)
    }

    // Status. The only hues in the interface.
    static let jade = Color(light: "#127A56", dark: "#45C596")
    static let jade2 = Color(light: "#16906A", dark: "#38B487")
    static let jade3 = Color(light: "#12B189", dark: "#57DDB2")
    static let jadeWash = Color(light: "#E3F1EB", dark: "#10312A")

    /// "Handed to the program" — a weaker guarantee, never an error. There is
    /// deliberately no red in the palette (§2).
    static let amber = Color(light: "#9A5615", dark: "#D89B5C")
    static let amberWash = Color(light: "#F8EDDF", dark: "#33261A")

    // Surfaces.
    static let surface = Color(light: "#FFFFFF", dark: "#1D1F20")
    static let surface2 = Color(light: "#F8F8F7", dark: "#191B1C")
    static let surface3 = Color(light: "#F1F1F0", dark: "#232527")
    static let hover = Color(light: "#F4F4F3", dark: "#26292A")
    static let selected = Color(light: "#E9EDEB", dark: "#2A3230")
    static let line = Color(light: "#E6E6E4", dark: "#2C2F30")
    static let line2 = Color(light: "#D5D5D2", dark: "#3A3D3E")

    // Ink.
    static let ink = Color(light: "#16191A", dark: "#F0F1F1")
    static let ink2 = Color(light: "#61666A", dark: "#9BA1A4")
    static let ink3 = Color(light: "#90969A", dark: "#6E7477")
}

/// DESIGN.md §5 — the 4px base grid. Named so a stray `padding(13)` stands out
/// in review as something that had to be argued for.
enum Space {
    static let xs: CGFloat = 4
    static let s: CGFloat = 8
    static let m: CGFloat = 12
    static let l: CGFloat = 16
    static let xl: CGFloat = 20
}

/// DESIGN.md §5 — radii climb with the size of the thing they round.
enum Radius {
    static let key: CGFloat = 4
    static let control: CGFloat = 6
    static let input: CGFloat = 7
    static let avatar: CGFloat = 8
    static let card: CGFloat = 9
    static let window: CGFloat = 11
    static let popover: CGFloat = 12
}

/// DESIGN.md §3 — type scale. Tracking is negative and scales with size; SF is
/// optically loose at display sizes and needs less tightening as it shrinks.
enum Kind {
    static func heading(_ size: CGFloat = 18) -> Font { .system(size: size, weight: .semibold) }
    static let rowTitle = Font.system(size: 13.5, weight: .medium)
    static let body = Font.system(size: 13)
    static let label = Font.system(size: 12, weight: .medium)
    static let meta = Font.system(size: 11.5)
    static let mono = Font.system(size: 11, design: .monospaced)

    static let rowTitleTracking: CGFloat = -0.16
    static let headingTracking: CGFloat = -0.5
    static let metaTracking: CGFloat = -0.03
}

/// The three states a secret can be in. Order and meaning come from
/// ARCHITECTURE.md §4 — the user never picks one, Keyward reports it.
enum SecretStatus: String, Codable {
    case protected
    case shared
    case unused

    var dot: Color {
        switch self {
        case .protected: Tint.jade2
        case .shared: Tint.amber
        case .unused: Tint.line2
        }
    }

    var halo: Color {
        switch self {
        case .protected: Tint.jadeWash
        case .shared: Tint.amberWash
        case .unused: .clear
        }
    }

    var label: LocalizedStringKey {
        switch self {
        case .protected: "status.protected"
        case .shared: "status.shared"
        case .unused: "status.unused"
        }
    }
}

/// A session that is forwarding right now: the same 7px dot with a halo that
/// breathes. Motion is reserved for this one thing — it is the only state in the
/// product that changes while you watch it.
struct LiveDot: View {
    @State private var expanded = false

    var body: some View {
        Circle()
            .fill(Tint.jade2)
            .frame(width: 7, height: 7)
            .overlay(
                Circle()
                    .stroke(Tint.jade2.opacity(expanded ? 0 : 0.5), lineWidth: 2)
                    .scaleEffect(expanded ? 2.6 : 1)
            )
            .onAppear {
                withAnimation(.easeOut(duration: 1.4).repeatForever(autoreverses: false)) {
                    expanded = true
                }
            }
            .accessibilityLabel(Text("live.forwarding"))
    }
}

/// The 7px status dot with its 3px wash halo (DESIGN.md §4).
struct StatusDot: View {
    let status: SecretStatus

    var body: some View {
        Circle()
            .fill(status.dot)
            .frame(width: 7, height: 7)
            .overlay(Circle().stroke(status.halo, lineWidth: 3))
            .accessibilityLabel(Text(status.label))
    }
}
