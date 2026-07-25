//
//  WardMark.swift
//  The logo: the ward — the notched profile inside a lock that decides what
//  passes. Not a key, not a shield, not a padlock (DESIGN.md §4).
//

import SwiftUI

/// The mark, drawn in a 14×26 design box and scaled to fit, preserving aspect.
///
/// The bulb is deliberately **wider than the stem**. An earlier version made them
/// the same width and the silhouette read as a rounded rectangle with bites out of
/// it — geometrically the same idea, visually a block. The keyhole only reads as a
/// keyhole when the head overhangs the shaft.
///
/// Two renditions, because two notches turn to mush below about 20pt. Shipping a
/// second path for small sizes is normal icon practice, not a compromise.
struct WardMark: Shape {
    enum Size {
        /// ≥24pt — two notches.
        case large
        /// ≤20pt — one notch. Menu bar, popover header.
        case small
    }

    var size: Size = .large

    /// Where the stem's edge meets the bulb: `7 - sqrt(7² - 3²)` below centre.
    private static let junctionY: CGFloat = 13.3246
    /// The same junction expressed as angles about the bulb's centre. The sweep
    /// runs to `360 + 64.62` so it takes the major arc over the top rather than
    /// cutting straight across.
    private static let junctionStart: CGFloat = 115.38
    private static let junctionEnd: CGFloat = 424.62

    func path(in rect: CGRect) -> Path {
        let scale = min(rect.width / 14, rect.height / 26)
        let originX = rect.midX - 7 * scale
        let originY = rect.midY - 13 * scale
        func pt(_ x: CGFloat, _ y: CGFloat) -> CGPoint {
            CGPoint(x: originX + x * scale, y: originY + y * scale)
        }

        var path = Path()
        path.move(to: pt(4, Self.junctionY))
        // In SwiftUI's y-down space an increasing angle reads clockwise on screen,
        // so `clockwise: false` sweeps over the top.
        path.addArc(
            center: pt(7, 7),
            radius: 7 * scale,
            startAngle: .degrees(Self.junctionStart),
            endAngle: .degrees(Self.junctionEnd),
            clockwise: false
        )

        let stem: [(CGFloat, CGFloat)] =
            switch size {
            case .large:
                [(10, 16), (7, 16), (7, 18), (10, 18),
                 (10, 21), (7, 21), (7, 23), (10, 23),
                 (10, 26), (4, 26)]
            case .small:
                [(10, 17), (7, 17), (7, 20), (10, 20), (10, 26), (4, 26)]
            }
        for (x, y) in stem { path.addLine(to: pt(x, y)) }

        path.closeSubpath()
        return path
    }
}

/// The app icon: squircle, graphite gradient, gloss on the top half, and the mark
/// at 60% of the icon's height (DESIGN.md §4 — an earlier draft had it at ~30% and
/// it read as a scratch).
///
/// Kept in sync by hand with `apps/mac/tools`-generated PNGs in the asset
/// catalogue: this view is what the settings screen shows, the catalogue is what
/// the Dock shows, and they are two renderings of the same gradient.
struct AppMark: View {
    var side: CGFloat = 64

    private var shape: RoundedRectangle {
        RoundedRectangle(cornerRadius: side * 0.23, style: .continuous)
    }

    var body: some View {
        shape
            .fill(
                LinearGradient(
                    colors: [Color(light: "#4A4F55", dark: "#5A6068"), Color(light: "#24272B", dark: "#2E3238"), Color(light: "#111315", dark: "#17191C")],
                    startPoint: .topLeading,
                    endPoint: .bottomTrailing
                )
            )
            .overlay(
                LinearGradient(
                    colors: [.white.opacity(0.22), .white.opacity(0)],
                    startPoint: .top,
                    endPoint: .center
                )
                .clipShape(shape)
            )
            .overlay(
                WardMark(size: .large)
                    .fill(.white)
                    .frame(height: side * 0.60)
                    .shadow(color: .black.opacity(0.25), radius: 1, y: 1)
            )
            .frame(width: side, height: side)
            .shadow(color: .black.opacity(0.18), radius: 10, y: 4)
    }
}

#Preview("Mark renditions") {
    HStack(alignment: .bottom, spacing: 24) {
        AppMark(side: 64)
        WardMark(size: .large).fill(Tint.ink).frame(width: 14, height: 26)
        WardMark(size: .small).fill(Tint.ink).frame(width: 9, height: 17)
    }
    .padding(32)
}
