import Foundation

public enum Mode: String, Codable, CaseIterable, Sendable {
    case off, focus, strict
    case onBreak = "break"

    public var label: String {
        switch self {
        case .off: return "Off"
        case .focus: return "Focus"
        case .onBreak: return "Break"
        case .strict: return "Strict"
        }
    }
    /// Site rules, app rules and Jev apply only in these modes.
    public var blocks: Bool { self == .focus || self == .strict }
}

public enum SwitchBehavior: String, Codable, CaseIterable, Sendable {
    case leave, hide
    public var label: String { self == .leave ? "Leave other apps alone" : "Hide apps outside the group" }
}

public struct AppGroup: Codable, Identifiable, Hashable, Sendable {
    public var id = UUID()
    public var name: String
    public var icon: String = "square.grid.2x2"
    public var colorHex: String = "#4F7CFF"
    /// Bundle identifiers shown in Kairos Bar.
    public var dockApps: [String] = []
    public var blockedApps: [String] = []
    public var allowSites: [String] = []
    public var blockSites: [String] = []
    /// hostname -> CSS selectors hidden on that site.
    public var hiddenElements: [String: [String]] = [:]
    public var jevEnabled: Bool = true
    public var jevThreshold: Double = 0.7
    public var tempoPreset: String = "Classic"
    /// Mission Control desktop (1-9) to switch to with this group. 0 leaves the desktop alone.
    public var desktop: Int = 0

    public init(name: String, icon: String = "square.grid.2x2", colorHex: String = "#4F7CFF") {
        self.name = name
        self.icon = icon
        self.colorHex = colorHex
    }

    public mutating func hideElement(_ selector: String, on host: String) {
        guard !(hiddenElements[host] ?? []).contains(selector) else { return }
        hiddenElements[host, default: []].append(selector)
    }

    /// Drops the host's entry once its last selector goes.
    public mutating func showElement(_ selector: String, on host: String) {
        guard var list = hiddenElements[host] else { return }
        list.removeAll { $0 == selector }
        hiddenElements[host] = list.isEmpty ? nil : list
    }
}

extension AppGroup: Lenient {
    public static var fallback: AppGroup { AppGroup(name: "") }
    public static let fields: [LenientField<AppGroup>] = [
        .required("name", \.name), .field("id", \.id), .field("icon", \.icon), .field("colorHex", \.colorHex),
        .field("dockApps", \.dockApps), .field("blockedApps", \.blockedApps), .field("allowSites", \.allowSites),
        .field("blockSites", \.blockSites), .field("hiddenElements", \.hiddenElements), .field("jevEnabled", \.jevEnabled),
        .field("jevThreshold", \.jevThreshold), .field("tempoPreset", \.tempoPreset), .field("desktop", \.desktop),
    ]
}

public enum VoiceProvider: String, Codable, CaseIterable, Sendable {
    case system, elevenlabs, openai
    public var label: String {
        switch self {
        case .system: return "Mac voices (free)"
        case .elevenlabs: return "ElevenLabs"
        case .openai: return "OpenAI"
        }
    }
    /// Keychain account holding this provider's API key.
    public var keyAccount: String? { self == .system ? nil : rawValue }
}

public enum TaskStatus: String, Codable, Sendable { case todo, done }

public struct TaskItem: Codable, Identifiable, Hashable, Sendable {
    public var id = UUID()
    public var title: String
    public var notes: String = ""
    public var groupId: UUID?
    /// 0 none, 1 low, 2 medium, 3 high
    public var priority: Int = 0
    public var due: Date?
    public var estimateMin: Int?
    public var status: TaskStatus = .todo
    public var parentId: UUID?
    public var createdAt = Date()
    public var completedAt: Date?
    public var listenedSec: Int = 0

    public init(title: String, groupId: UUID? = nil) {
        self.title = title
        self.groupId = groupId
    }
}

extension TaskItem: Lenient {
    public static var fallback: TaskItem { TaskItem(title: "") }
    public static let fields: [LenientField<TaskItem>] = [
        .required("title", \.title), .field("id", \.id), .field("notes", \.notes), .field("groupId", \.groupId),
        .field("priority", \.priority), .field("due", \.due), .field("estimateMin", \.estimateMin), .field("status", \.status),
        .field("parentId", \.parentId), .field("createdAt", \.createdAt), .field("completedAt", \.completedAt),
        .field("listenedSec", \.listenedSec),
    ]
}

public enum SessionKind: String, Codable, Sendable { case focus
    case rest = "break" }

public struct Session: Codable, Identifiable, Hashable, Sendable {
    public var id = UUID()
    public var taskId: UUID?
    public var groupId: UUID?
    public var start: Date
    public var end: Date
    public var kind: SessionKind
    public var interruptions: Int = 0
    public var completed: Bool = true

    public init(taskId: UUID?, groupId: UUID?, start: Date, end: Date, kind: SessionKind, interruptions: Int = 0, completed: Bool = true) {
        self.taskId = taskId; self.groupId = groupId; self.start = start; self.end = end
        self.kind = kind; self.interruptions = interruptions; self.completed = completed
    }
    public var minutes: Double { max(0, end.timeIntervalSince(start)) / 60 }
}

extension Session: Lenient {
    public static var fallback: Session { Session(taskId: nil, groupId: nil, start: .distantPast, end: .distantPast, kind: .focus) }
    public static let fields: [LenientField<Session>] = [
        .required("start", \.start), .required("end", \.end), .field("id", \.id), .field("taskId", \.taskId),
        .field("groupId", \.groupId), .field("kind", \.kind), .field("interruptions", \.interruptions),
        .field("completed", \.completed),
    ]
}

public struct BlockEvent: Codable, Identifiable, Hashable, Sendable {
    public var id = UUID()
    public var time = Date()
    public var groupId: UUID?
    /// Hostname for sites, bundle id for apps.
    public var target: String
    /// What to show for `target`: the app's name. Nil for sites, where the hostname reads fine.
    public var label: String?
    /// "rule", "jev" or "app"
    public var layer: String
    public var overridden: Bool = false

    public init(groupId: UUID?, target: String, label: String? = nil, layer: String, overridden: Bool = false, time: Date = Date()) {
        self.groupId = groupId; self.target = target; self.label = label; self.layer = layer
        self.overridden = overridden; self.time = time
    }

    public var displayName: String { label ?? target }
}

extension BlockEvent: Lenient {
    public static var fallback: BlockEvent { BlockEvent(groupId: nil, target: "", layer: "rule", time: .distantPast) }
    public static let fields: [LenientField<BlockEvent>] = [
        .required("target", \.target), .required("time", \.time), .field("id", \.id), .field("groupId", \.groupId),
        .field("label", \.label), .field("layer", \.layer), .field("overridden", \.overridden),
    ]
}

public struct TempoPreset: Codable, Identifiable, Hashable, Sendable {
    public var id: String { name }
    public var name: String
    public var focusMin: Int
    public var shortBreakMin: Int
    public var longBreakMin: Int
    public var cyclesBeforeLong: Int = 4
    public var strict: Bool = false

    public init(name: String, focusMin: Int, shortBreakMin: Int, longBreakMin: Int, cyclesBeforeLong: Int = 4, strict: Bool = false) {
        self.name = name; self.focusMin = focusMin; self.shortBreakMin = shortBreakMin
        self.longBreakMin = longBreakMin; self.cyclesBeforeLong = cyclesBeforeLong; self.strict = strict
    }

    public static let defaults: [TempoPreset] = [
        .init(name: "Classic", focusMin: 25, shortBreakMin: 5, longBreakMin: 15),
        .init(name: "Deep work", focusMin: 50, shortBreakMin: 10, longBreakMin: 30, strict: true),
        .init(name: "Sprint", focusMin: 15, shortBreakMin: 3, longBreakMin: 10),
    ]
}

extension TempoPreset: Lenient {
    public static var fallback: TempoPreset { TempoPreset(name: "", focusMin: 25, shortBreakMin: 5, longBreakMin: 15) }
    public static let fields: [LenientField<TempoPreset>] = [
        .required("name", \.name), .field("focusMin", \.focusMin), .field("shortBreakMin", \.shortBreakMin),
        .field("longBreakMin", \.longBreakMin), .field("cyclesBeforeLong", \.cyclesBeforeLong), .field("strict", \.strict),
    ]
}

public enum DockEdge: String, Codable, CaseIterable, Sendable { case left, right, bottom }

public struct Settings: Codable, Hashable, Sendable {
    public var dockEdge: DockEdge = .left
    public var dockVisible: Bool = true
    public var iconSize: Double = 36
    public var switchBehavior: SwitchBehavior = .leave
    public var presets: [TempoPreset] = TempoPreset.defaults
    public var neverAnalyse: [String] = Settings.defaultNeverAnalyse
    /// When Jev is slow or offline: false lets the page through, true blocks it.
    public var jevFailClosed: Bool = false
    public var maxOverridesPerSession: Int = 3
    public var overrideMinutes: Int = 5
    public var breakOverlay: Bool = true
    /// Control-1...9 switch groups. Off if it clashes with Mission Control shortcuts.
    /// ⌃1 to ⌃9 switch groups. Off for new profiles: macOS uses them for "Switch to Desktop".
    public var groupHotkeys: Bool = false
    /// A `Hotkeys.quickAdd` id, or "off".
    public var quickAddHotkey: String = "option-space"
    /// A `Hotkeys.timer` id, or "off".
    public var timerHotkey: String = "option-shift-space"
    public var voiceProvider: VoiceProvider = .system
    /// Voice id for the chosen cloud provider; empty means that provider's default.
    public var voiceId: String = ""
    /// Slide Kairos Bar off its edge while the pointer is away, like the Dock's auto-hide.
    public var autoHideBar: Bool = false
    /// Keep the macOS Dock hidden while Kairos Bar is on screen.
    public var replacesSystemDock: Bool = false
    /// A notification when a task's due time comes.
    public var notifiesWhenDue: Bool = true
    /// Downloads and Trash at the end of Kairos Bar.
    public var showsDockFolders: Bool = true
    /// After a break runs out, start the next focus phase at once instead of waiting.
    public var startsFocusAfterBreak: Bool = false
    /// In Focus and Strict, hide a browser that Kairos Shield is not running in.
    public var blockUnshieldedBrowsers: Bool = true
    /// While Kairos Bar is pinned open, move or shrink windows that would sit under it.
    public var keepsWindowsClear: Bool = true

    public init() {}

    public static let defaultNeverAnalyse = [
        "mail.google.com", "outlook.live.com", "outlook.office.com", "proton.me", "icloud.com",
        "paypal.com", "chase.com", "bankofamerica.com", "wellsfargo.com", "hsbc.com", "barclays.co.uk",
        "monzo.com", "revolut.com", "nhs.uk", "mychart.com", "patient.info", "1password.com",
    ]
}

extension Settings {
    /// A voice id belongs to one provider, so choosing another provider starts from its default voice.
    public mutating func setVoiceProvider(_ provider: VoiceProvider) {
        guard provider != voiceProvider else { return }
        voiceProvider = provider
        voiceId = ""
    }
}

extension Settings: Lenient {
    public static var fallback: Settings { Settings() }
    public static let fields: [LenientField<Settings>] = [
        .field("dockEdge", \.dockEdge), .field("dockVisible", \.dockVisible), .field("iconSize", \.iconSize),
        .field("switchBehavior", \.switchBehavior), .field("presets", \.presets), .field("neverAnalyse", \.neverAnalyse),
        .field("jevFailClosed", \.jevFailClosed), .field("maxOverridesPerSession", \.maxOverridesPerSession),
        .field("overrideMinutes", \.overrideMinutes), .field("breakOverlay", \.breakOverlay),
        .field("groupHotkeys", \.groupHotkeys), .field("quickAddHotkey", \.quickAddHotkey), .field("timerHotkey", \.timerHotkey), .field("voiceProvider", \.voiceProvider), .field("voiceId", \.voiceId),
        .field("autoHideBar", \.autoHideBar), .field("replacesSystemDock", \.replacesSystemDock),
        .field("blockUnshieldedBrowsers", \.blockUnshieldedBrowsers), .field("startsFocusAfterBreak", \.startsFocusAfterBreak), .field("showsDockFolders", \.showsDockFolders),
        .field("notifiesWhenDue", \.notifiesWhenDue), .field("keepsWindowsClear", \.keepsWindowsClear),
    ]
}

public struct FocusState: Codable, Hashable, Sendable {
    public var activeGroupId: UUID?
    public var currentTaskId: UUID?
    public var mode: Mode = .off
    public var endsAt: Date?
    /// Group id -> the task that was current when that group was last left.
    public var taskByGroup: [String: UUID] = [:]
    /// While a strict focus phase is locked: when it ends (or, paused, would end if resumed now).
    /// Saved so that quitting or a crash cannot end it, and pushed so Chrome keeps enforcing it.
    public var strictUntil: Date?
    public init() {}
}

extension FocusState: Lenient {
    public static var fallback: FocusState { FocusState() }
    public static let fields: [LenientField<FocusState>] = [
        .field("activeGroupId", \.activeGroupId), .field("currentTaskId", \.currentTaskId), .field("mode", \.mode),
        .field("endsAt", \.endsAt), .field("strictUntil", \.strictUntil), .field("taskByGroup", \.taskByGroup),
    ]
}

/// Everything Kairos persists, in one JSON file.
public struct AppData: Codable, Sendable {
    public var groups: [AppGroup] = []
    public var tasks: [TaskItem] = []
    public var sessions: [Session] = []
    public var blockEvents: [BlockEvent] = []
    public var settings = Settings()
    public var focus = FocusState()
    public var onboarded = false
    /// Extension clients ("shield", "read") that have connected at least once, for the setup list.
    public var seenClients: [String] = []
    /// The owner hid the setup list on Today.
    public var setupHidden = false
    /// The timer phase running at the last save, kept so a quit or crash still logs it.
    public var openSession: Session?
    /// Kairos Read listening time while no task was current.
    public var unassignedListenedSec: Int = 0

    public init() {}

    /// `browser` takes Safari's place in the groups' apps: site rules only reach a browser that
    /// can run Kairos Shield.
    public static func starter(browser: String? = nil) -> AppData {
        var d = AppData()
        var work = AppGroup(name: "Work", icon: "briefcase.fill", colorHex: "#4F7CFF")
        work.dockApps = ["com.apple.mail", "com.apple.Safari", "com.apple.Notes", "com.apple.iCal"]
        work.blockedApps = ["com.hnc.Discord", "com.spotify.client", "com.apple.TV"]
        work.blockSites = ["youtube.com", "reddit.com", "x.com", "twitter.com", "instagram.com", "tiktok.com", "netflix.com"]
        var study = AppGroup(name: "Study", icon: "book.fill", colorHex: "#2FA36B")
        study.dockApps = ["com.apple.Safari", "com.apple.Notes", "com.apple.Preview"]
        study.blockedApps = ["com.hnc.Discord", "com.apple.mail", "com.tinyspeck.slackmacgap"]
        study.blockSites = ["x.com", "twitter.com", "instagram.com", "tiktok.com", "reddit.com"]
        var personal = AppGroup(name: "Personal", icon: "house.fill", colorHex: "#E0703A")
        personal.dockApps = ["com.apple.MobileSMS", "com.apple.Photos", "com.apple.Music"]
        personal.jevEnabled = false
        if let browser {
            work.dockApps = work.dockApps.map { $0 == "com.apple.Safari" ? browser : $0 }
            study.dockApps = study.dockApps.map { $0 == "com.apple.Safari" ? browser : $0 }
        }
        d.groups = [work, study, personal]
        d.focus.activeGroupId = work.id
        return d
    }

    public func group(_ id: UUID?) -> AppGroup? { groups.first { $0.id == id } }
    public var activeGroup: AppGroup? { group(focus.activeGroupId) ?? groups.first }
    public var currentTask: TaskItem? { tasks.first { $0.id == focus.currentTaskId } }
    public func preset(named name: String) -> TempoPreset {
        settings.presets.first { $0.name == name } ?? settings.presets.first ?? TempoPreset.defaults[0]
    }
}

extension AppData: Lenient {
    public static var fallback: AppData { AppData() }
    public static let fields: [LenientField<AppData>] = [
        .field("groups", \.groups), .field("tasks", \.tasks), .field("sessions", \.sessions),
        .field("blockEvents", \.blockEvents), .field("settings", \.settings), .field("focus", \.focus),
        .field("onboarded", \.onboarded), .field("openSession", \.openSession),
        .field("unassignedListenedSec", \.unassignedListenedSec),
        .field("seenClients", \.seenClients), .field("setupHidden", \.setupHidden),
    ]
}
