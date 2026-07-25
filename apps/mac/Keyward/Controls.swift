//
//  Controls.swift
//  Form controls that follow DESIGN.md instead of AppKit's defaults.
//
//  The sheets were built from stock `.roundedBorder` fields and default push
//  buttons, so they arrived in system blue with a system focus ring — a screen
//  that had never been designed, sitting on top of one that had.
//

import SwiftUI

/// Label, control, and an optional hint, at the spacing the detail pane uses.
struct FormField<Control: View>: View {
    let label: LocalizedStringKey
    var hint: LocalizedStringKey?
    @ViewBuilder var control: Control

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(label)
                .font(.system(size: 11.5, weight: .medium))
                .foregroundStyle(Tint.ink2)
            control
            if let hint {
                Text(hint)
                    .font(Kind.meta)
                    .foregroundStyle(Tint.ink3)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}

/// A text field with the app's own chrome: `surface3` fill, hairline border, and
/// a neutral focus ring rather than the system accent.
struct KeyTextField: View {
    @Binding var text: String
    var secure = false
    var mono = false
    var readOnly = false
    var placeholder: LocalizedStringKey?

    @FocusState private var focused: Bool

    var body: some View {
        Group {
            if readOnly {
                Text(text)
                    .font(font)
                    .foregroundStyle(text.isEmpty ? Tint.ink3 : Tint.ink)
                    .frame(maxWidth: .infinity, alignment: .leading)
            } else if secure {
                SecureField("", text: $text)
                    .textFieldStyle(.plain)
                    .font(font)
                    .focused($focused)
            } else {
                TextField(placeholder ?? "", text: $text)
                    .textFieldStyle(.plain)
                    .font(font)
                    .focused($focused)
            }
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 8)
        .background(
            RoundedRectangle(cornerRadius: Radius.input)
                .fill(readOnly ? Tint.surface3 : Tint.surface)
        )
        .overlay(
            RoundedRectangle(cornerRadius: Radius.input)
                .strokeBorder(focused ? Tint.accent : Tint.line2, lineWidth: focused ? 1.5 : 0.5)
        )
        .background(
            RoundedRectangle(cornerRadius: Radius.input)
                .fill(Tint.accentWash)
                .opacity(focused ? 1 : 0)
                .padding(-3)
        )
        .animation(.easeOut(duration: 0.12), value: focused)
    }

    private var font: Font {
        mono ? .system(size: 12.5, design: .monospaced) : .system(size: 13)
    }
}

/// The filled graphite action. Same treatment as the toolbar's `Add secret`, so
/// the primary control looks the same wherever it appears.
struct PrimaryButtonStyle: ButtonStyle {
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: 13, weight: .medium))
            .foregroundStyle(isEnabled ? Tint.onAccent : Tint.ink3)
            .padding(.horizontal, 16)
            .padding(.vertical, 7)
            .background(
                RoundedRectangle(cornerRadius: Radius.control)
                    .fill(isEnabled ? AnyShapeStyle(Tint.accentFill) : AnyShapeStyle(Tint.surface3))
            )
            .overlay(
                RoundedRectangle(cornerRadius: Radius.control)
                    .strokeBorder(
                        isEnabled ? .white.opacity(0.18) : Tint.line2,
                        lineWidth: 0.5
                    )
            )
            .opacity(configuration.isPressed && isEnabled ? 0.82 : 1)
            .shadow(color: .black.opacity(isEnabled ? 0.13 : 0), radius: 1.5, y: 1)
    }
}

/// The quiet counterpart — cancel, and anything secondary.
struct GhostButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: 13))
            .foregroundStyle(Tint.ink)
            .padding(.horizontal, 14)
            .padding(.vertical, 7)
            .background(
                RoundedRectangle(cornerRadius: Radius.control)
                    .fill(configuration.isPressed ? Tint.hover : Tint.surface)
            )
            .overlay(
                RoundedRectangle(cornerRadius: Radius.control)
                    .strokeBorder(Tint.line2, lineWidth: 0.5)
            )
            .shadow(color: .black.opacity(0.05), radius: 1, y: 0.5)
    }
}

/// Shared chrome for the two sheets: title, content, and a right-aligned button
/// row, so `Add` and `Replace` are the same screen with different fields.
struct SheetFrame<Content: View, Actions: View>: View {
    let title: LocalizedStringKey
    @ViewBuilder var content: Content
    @ViewBuilder var actions: Actions

    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            Text(title)
                .font(.system(size: 19, weight: .semibold))
                .tracking(Kind.headingTracking)
                .foregroundStyle(Tint.ink)

            VStack(alignment: .leading, spacing: 16) {
                content
            }

            HStack(spacing: 8) {
                Spacer()
                actions
            }
            .padding(.top, 4)
        }
        .padding(24)
        .frame(width: 400)
        .background(Tint.surface)
    }
}
