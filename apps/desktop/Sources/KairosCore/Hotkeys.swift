import Foundation

/// A global shortcut the owner can pick in Settings. Key codes and modifier flags are Carbon's
/// (`kVK_*`, `optionKey` and so on), which RegisterEventHotKey takes.
public struct HotkeyChoice: Hashable, Sendable, Identifiable {
    /// Stored in Settings.
    public let id: String
    /// As macOS menus show it, such as "⌥Space".
    public let label: String
    public let keyCode: UInt32
    public let modifiers: UInt32
    /// The key as typed, for showing the shortcut in a menu ("" for Space is " ").
    public let character: Character

    public static let command: UInt32 = 0x0100, shift: UInt32 = 0x0200, option: UInt32 = 0x0800, control: UInt32 = 0x1000
    static let space: UInt32 = 49, keyN: UInt32 = 45, keyT: UInt32 = 17, keyP: UInt32 = 35
}

public enum Hotkeys {
    public static let off = "off"

    /// Add a task from anywhere.
    public static let quickAdd: [HotkeyChoice] = [
        HotkeyChoice(id: "option-space", label: "⌥Space", keyCode: HotkeyChoice.space, modifiers: HotkeyChoice.option, character: " "),
        HotkeyChoice(id: "control-option-space", label: "⌃⌥Space", keyCode: HotkeyChoice.space,
                     modifiers: HotkeyChoice.control | HotkeyChoice.option, character: " "),
        HotkeyChoice(id: "control-option-n", label: "⌃⌥N", keyCode: HotkeyChoice.keyN,
                     modifiers: HotkeyChoice.control | HotkeyChoice.option, character: "n"),
    ]

    /// Start, pause or resume the timer from anywhere.
    public static let timer: [HotkeyChoice] = [
        HotkeyChoice(id: "option-shift-space", label: "⌥⇧Space", keyCode: HotkeyChoice.space,
                     modifiers: HotkeyChoice.option | HotkeyChoice.shift, character: " "),
        HotkeyChoice(id: "control-option-t", label: "⌃⌥T", keyCode: HotkeyChoice.keyT,
                     modifiers: HotkeyChoice.control | HotkeyChoice.option, character: "t"),
        HotkeyChoice(id: "control-option-p", label: "⌃⌥P", keyCode: HotkeyChoice.keyP,
                     modifiers: HotkeyChoice.control | HotkeyChoice.option, character: "p"),
    ]

    /// The choice stored as `id`, or nil when it is off. An id this build does not know falls
    /// back to the first choice, so a shortcut never vanishes silently.
    public static func choice(_ id: String, in choices: [HotkeyChoice]) -> HotkeyChoice? {
        guard id != off else { return nil }
        return choices.first { $0.id == id } ?? choices.first
    }
}

/// The global shortcuts Kairos registers, from Settings. Re-registered whenever it changes.
public struct HotkeyPlan: Equatable, Sendable {
    public struct Entry: Equatable, Sendable {
        public enum Action: Equatable, Sendable { case quickAdd, timer, group(Int) }
        public let action: Action
        public let label: String
        public let keyCode: UInt32
        public let modifiers: UInt32
    }

    public let entries: [Entry]

    public init(_ settings: Settings) {
        var entries: [Entry] = []
        if let c = Hotkeys.choice(settings.quickAddHotkey, in: Hotkeys.quickAdd) {
            entries.append(Entry(action: .quickAdd, label: c.label, keyCode: c.keyCode, modifiers: c.modifiers))
        }
        if let c = Hotkeys.choice(settings.timerHotkey, in: Hotkeys.timer) {
            entries.append(Entry(action: .timer, label: c.label, keyCode: c.keyCode, modifiers: c.modifiers))
        }
        if settings.groupHotkeys {
            // kVK_ANSI_1 ... kVK_ANSI_9
            let digits: [UInt32] = [18, 19, 20, 21, 23, 22, 26, 28, 25]
            for (i, code) in digits.enumerated() {
                entries.append(Entry(action: .group(i), label: "⌃\(i + 1)", keyCode: code, modifiers: HotkeyChoice.control))
            }
        }
        self.entries = entries
    }

    /// What Settings says when some shortcuts could not be registered.
    public static func failureText(_ labels: [String]) -> String? {
        guard !labels.isEmpty else { return nil }
        let list = ListFormatter.localizedString(byJoining: labels)
        return labels.count == 1
            ? "\(list) is already taken by another app, so it does nothing in Kairos. Pick another shortcut or quit that app."
            : "\(list) are already taken by another app, so they do nothing in Kairos. Pick other shortcuts or quit that app."
    }
}
