import Foundation

/// The things the owner sets up once so Kairos can do everything it offers. Today lists them
/// until they are done or the owner hides the list.
public enum SetupStep: String, CaseIterable, Sendable {
    case shield, read, jevKey, claudeKey, voiceKey, accessibility, openAtLogin, dockReplacement
}

/// What Kairos knows about the Mac and the profile when it draws the list.
public struct SetupFacts: Sendable {
    /// Extension clients ("shield", "read") that have connected at least once.
    public var seenClients: Set<String> = []
    public var hasJevKey = false
    public var hasClaudeKey = false
    public var voiceProvider: VoiceProvider = .system
    public var hasVoiceKey = false
    public var accessibility = false
    public var openAtLogin = false
    public var replacesDock = false

    public init() {}
}

public struct SetupChecklist: Sendable {
    public struct Item: Identifiable, Equatable, Sendable {
        public let step: SetupStep
        public let done: Bool
        public let title: String
        public let detail: String
        public var id: SetupStep { step }
    }

    public let items: [Item]

    public init(_ facts: SetupFacts) {
        var items: [Item] = [
            Item(step: .shield, done: facts.seenClients.contains("shield"), title: "Load Kairos Shield",
                 detail: "Blocks sites and hides page elements. Without it Kairos can\u{2019}t block any site. In chrome://extensions turn on Developer mode, choose \u{201C}Load unpacked\u{201D} and pick the shield folder."),
            Item(step: .read, done: facts.seenClients.contains("read"), title: "Load Kairos Read (optional)",
                 detail: "Reads articles aloud and builds 3D scenes from them. Load the read folder the same way."),
            Item(step: .jevKey, done: facts.hasJevKey, title: "Add a TypeSafe key for Jev",
                 detail: "Jev checks whether a page or app fits your current task. Without it only your block and allow lists apply."),
            Item(step: .claudeKey, done: facts.hasClaudeKey, title: "Add an Anthropic key (optional)",
                 detail: "For 3D scenes in Kairos Read."),
        ]
        if facts.voiceProvider != .system {
            items.append(Item(step: .voiceKey, done: facts.hasVoiceKey, title: "Add your \(facts.voiceProvider.label) key",
                              detail: "You chose \(facts.voiceProvider.label) voices for Kairos Read. Until the key is saved it reads with Mac voices."))
        }
        items += [
            Item(step: .accessibility, done: facts.accessibility, title: "Allow Accessibility",
                 detail: "Lets Jev read the front window\u{2019}s title, lets Kairos switch desktops, and keeps windows from sitting under Kairos Bar while it stays on screen."),
            Item(step: .openAtLogin, done: facts.openAtLogin, title: "Open Kairos at login",
                 detail: "So blocking and Kairos Bar are there from the moment you log in."),
            Item(step: .dockReplacement, done: facts.replacesDock, title: "Use Kairos Bar instead of the Dock (optional)",
                 detail: "Kairos hides the macOS Dock while Kairos Bar is on screen and puts it back when you turn this off or quit."),
        ]
        self.items = items
    }

    public var doneCount: Int { items.filter(\.done).count }
    public var isComplete: Bool { doneCount == items.count }
}
