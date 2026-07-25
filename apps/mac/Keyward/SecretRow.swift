//
//  SecretRow.swift
//  The primary component (DESIGN.md §4). The product is a list; this row is where
//  the craft is judged.
//

import SwiftUI

struct SecretRow: View {
    let secret: SecretSummary
    var compact = false
    var isHovered = false
    var isSelected = false

    private var avatarSide: CGFloat { compact ? 28 : 32 }

    var body: some View {
        HStack(spacing: 11) {
            Avatar(letter: secret.letter, hex: secret.tint, logo: secret.logo, side: avatarSide)

            VStack(alignment: .leading, spacing: 1) {
                Text(secret.display)
                    .font(.system(size: 14, weight: .medium))
                    .tracking(Kind.rowTitleTracking)
                    .foregroundStyle(Tint.ink)
                    .lineLimit(1)

                Text(usage)
                    .font(.system(size: 12))
                    .tracking(Kind.metaTracking)
                    .foregroundStyle(Tint.ink3)
                    .lineLimit(1)
                    .truncationMode(.tail)
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            HStack(spacing: Space.s + 2) {
                if !compact, let masked = secret.masked {
                    Text(masked)
                        .font(Kind.mono)
                        .monospacedDigit()
                        .foregroundStyle(Tint.ink3)
                }
                // A live session outranks the resting status: while Keyward is
                // forwarding, "something is happening right now" is the more
                // useful thing for the dot to say.
                if secret.live != nil {
                    LiveDot()
                } else {
                    // The dot keeps its own colour when the row is selected. An
                    // earlier version turned it white there, which removed the
                    // only thing it was for: white means nothing, and the row
                    // lost its state exactly when the user was looking at it.
                    StatusDot(status: secret.status)
                }
                if !compact {
                    Image(systemName: "chevron.right")
                        .font(.system(size: 10, weight: .semibold))
                        .foregroundStyle(Tint.ink3)
                        .opacity(isHovered ? 1 : 0)
                        .offset(x: isHovered ? 0 : -3)
                }
            }
        }
        .padding(.horizontal, 15)
        .padding(.vertical, 11)
        .frame(minHeight: 52)
        .contentShape(Rectangle())
    }

    /// "codex in my-shop · 2 min ago". Assembled from localised fragments rather
    /// than a format string with three positional arguments, because the word
    /// order differs between the two languages we ship.
    private var usage: String {
        guard let use = secret.lastUse else {
            return Loc.t("row.neverUsed")
        }
        let when = use.at.relative
        if let project = use.project {
            return Loc.t("row.usedIn %@ %@ %@", use.actor, project, when)
        }
        return Loc.t("row.used %@ %@", use.actor, when)
    }
}

/// The service's mark when Keyward ships one, a monogram when it does not.
///
/// Brand logos sit on a **white chip in both themes**. Most brand SVGs are a single
/// dark colour (OpenAI's is `#0D0D0D`), so dropping them onto a dark surface makes
/// them vanish; a white chip is the convention every app with a brand-logo list
/// converges on, and Beacon already proved it out.
///
/// Monograms keep the gradient and inner highlight — a flat fill looks pasted on
/// (DESIGN.md §2). The tint belongs to the service, never to Keyward.
struct Avatar: View {
    let letter: String
    let hex: String
    var logo: String?
    var side: CGFloat = 28

    private var shape: RoundedRectangle {
        RoundedRectangle(cornerRadius: side * 0.29, style: .continuous)
    }

    var body: some View {
        Group {
            if let logo, !logo.isEmpty {
                shape
                    .fill(.white)
                    .overlay(shape.strokeBorder(.black.opacity(0.08), lineWidth: 0.5))
                    .overlay(
                        Image(logo)
                            .renderingMode(.original)
                            .resizable()
                            .aspectRatio(contentMode: .fit)
                            .frame(width: side * 0.62, height: side * 0.62)
                    )
            } else {
                shape
                    .fill(
                        LinearGradient(
                            colors: [base.opacity(0.86), base],
                            startPoint: .topLeading,
                            endPoint: .bottomTrailing
                        )
                    )
                    .overlay(shape.strokeBorder(.white.opacity(0.28), lineWidth: 0.5))
                    .overlay(
                        Text(letter)
                            .font(.system(size: side * 0.44, weight: .bold))
                            .foregroundStyle(.white)
                    )
            }
        }
        .frame(width: side, height: side)
    }

    private var base: Color { Color(light: hex, dark: hex) }
}
