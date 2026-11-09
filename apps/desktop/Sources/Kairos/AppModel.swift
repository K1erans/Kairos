import AppKit
import Combine
import KairosCore
import SwiftUI
import UserNotifications

/// What the block overlay is showing.
struct BlockNotice: Identifiable, Equatable {
    enum Kind: Equatable { case app(bundleId: String), rest }
    let id = UUID()
    var kind: Kind
    var title: String
    var detail: String
    /// The display showing the blocked app, so the overlay appears where the user is looking.
    var screen: CGDirectDisplayID?
}

struct ReadStatus: Equatable {
    var state: String
    var title: String
}

/// Owns all state. The main window, Kairos Bar, the menu bar item, the app guard and the
/// Chrome bridge all read from and write to this one object.
@MainActor
final class AppModel: ObservableObject {
    @Published var data: AppData { didSet { dataChanged(from: oldValue) } }
    @Published private(set) var tempo: TempoEngine
    @Published private(set) var now = Date()
    @Published private(set) var bridgeClients: [String] = []
    @Published private(set) var readStatus: ReadStatus?
    @Published var suggestion: TaskItem?
    @Published var notice: BlockNotice?
    @Published private(set) var hasJevKey = false
    @Published private(set) var hasClaudeKey = false
    @Published private(set) var voiceKeys: Set<String> = []
    @Published var cloudVoices: [CloudVoice] = []
    @Published private(set) var lastVoiceError: String?
    @Published private(set) var lastJevError: String?
    @Published private(set) var runningApps: Set<String> = []
    /// Open apps that show in a Dock (regular activation policy), in launch order, without Kairos.
    @Published private(set) var openApps: [String] = []
    /// Main window navigation, here so the dock and menu bar can route into it.
    @Published var page: Page? = .today
    @Published var selectedGroupId: UUID?
    /// A group whose name field the Groups page should focus once it shows it.
    @Published var renameGroupId: UUID?

    @Published private(set) var ledger = OverrideLedger()
    private let store: DataStore
    private let bridge = BridgeServer()
    private let jev = JevClient()
    private let scenes = SceneWriter()
    private let voices = VoiceClient()
    private let speech = SpeechPlayer()
    private var speechGeneration = 0
    private var timer: Timer?
    private var saveWork: DispatchWorkItem?
    private var lastSave = Date.distantPast
    private var lastPushed = Data()
    private var hiddenBySwitch: [UUID: [String]] = [:]
    private var observers: [NSObjectProtocol] = []

    /// `readKeys: false` skips the Keychain, so the snapshot tool never triggers an access prompt.
    init(store: DataStore = .standard(), readKeys: Bool = true) {
        self.store = store
        var loaded = store.load()
        loaded.recoverAfterLaunch()
        data = loaded
        tempo = TempoEngine(preset: loaded.preset(named: loaded.activeGroup?.tempoPreset ?? "Classic"))
        if readKeys { refreshKeys() }
    }

    // MARK: Lifecycle

    func start() {
        bridge.onMessage = { [weak self] client, message in
            Task { @MainActor in self?.handle(message, from: client) }
        }
        bridge.onClientsChanged = { [weak self] in
            Task { @MainActor in
                guard let self else { return }
                self.bridgeClients = self.bridge.clientNames
                if !self.bridgeClients.contains("read") { self.readStatus = nil }
            }
        }
        do { try bridge.start() } catch { NSLog("Kairos: bridge failed to start: \(error.localizedDescription)") }

        timer = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.tick() }
        }
        let wc = NSWorkspace.shared.notificationCenter
        observers.append(wc.addObserver(forName: NSWorkspace.didWakeNotification, object: nil, queue: .main) { [weak self] _ in
            Task { @MainActor in self?.tick() }
        })
        observers.append(wc.addObserver(forName: NSWorkspace.didActivateApplicationNotification, object: nil, queue: .main) { [weak self] note in
            let app = note.userInfo?[NSWorkspace.applicationUserInfoKey] as? NSRunningApplication
            Task { @MainActor in if let app { self?.appActivated(app) } }
        })
        for name in [NSWorkspace.didLaunchApplicationNotification, NSWorkspace.didTerminateApplicationNotification] {
            observers.append(wc.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
                Task { @MainActor in self?.refreshRunning() }
            })
        }
        refreshRunning()
        saveNow() // keeps what launch recovery logged
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound]) { _, _ in }
    }

    func shutdown() {
        bridge.stop()
        saveNow()
    }

    private func refreshRunning() {
        let running = NSWorkspace.shared.runningApplications
        runningApps = Set(running.compactMap(\.bundleIdentifier))
        let open = running.filter { $0.activationPolicy == .regular }.compactMap(\.bundleIdentifier)
            .filter { $0 != Bundle.main.bundleIdentifier }
        if open != openApps { openApps = open }
    }

    private func dataChanged(from old: AppData) {
        if old.focus.mode != data.focus.mode { modeChanged(from: old.focus.mode, to: data.focus.mode) }
        saveWork?.cancel()
        let work = DispatchWorkItem { [weak self] in self?.saveNow() }
        saveWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5, execute: work)
        pushState()
    }

    /// Saves with the running phase as `openSession`, so a quit or crash can still log it.
    private func saveNow() {
        let t = Date()
        var snapshot = data
        snapshot.openSession = tempo.current(now: t).flatMap {
            Session($0, taskId: data.focus.currentTaskId, groupId: data.focus.activeGroupId)
        }
        lastSave = t
        do { try store.save(snapshot) } catch { NSLog("Kairos: save failed: \(error.localizedDescription)") }
    }

    func exportJSON() -> Data? { try? DataStore.encoder.encode(data) }

    // MARK: Keys

    func refreshKeys() {
        hasJevKey = !(Keychain.get("typesafe") ?? "").isEmpty
        hasClaudeKey = !(Keychain.get("anthropic") ?? "").isEmpty
        voiceKeys = Set(["elevenlabs", "openai"].filter { !(Keychain.get($0) ?? "").isEmpty })
    }

    /// A cloud voice is usable when a provider is chosen and its key is saved.
    var cloudVoiceReady: Bool {
        guard let account = data.settings.voiceProvider.keyAccount else { return false }
        return voiceKeys.contains(account)
    }

    func loadCloudVoices() {
        switch data.settings.voiceProvider {
        case .system: cloudVoices = []
        case .openai: cloudVoices = VoiceClient.openAIVoices.map { CloudVoice(id: $0, name: $0.capitalized) }
        case .elevenlabs:
            guard let key = Keychain.get("elevenlabs"), !key.isEmpty else { cloudVoices = []; return }
            Task {
                do { self.cloudVoices = try await voices.elevenLabsVoices(apiKey: key); self.lastVoiceError = nil }
                catch { self.lastVoiceError = error.localizedDescription }
            }
        }
    }

    func setKey(_ value: String, account: String) {
        Keychain.set(value.trimmingCharacters(in: .whitespacesAndNewlines), for: account)
        refreshKeys()
        pushState()
    }

    // MARK: Derived

    var activeGroup: AppGroup? { data.activeGroup }
    var currentTask: TaskItem? { data.currentTask }
    var mode: Mode { data.focus.mode }
    var remaining: TimeInterval { tempo.remaining(now: now) }
    var progress: Double { tempo.progress(now: now) }
    var overridesLeft: Int { ledger.remaining(max: data.settings.maxOverridesPerSession) }

    var menuTitle: String {
        let name = currentTask?.title ?? activeGroup?.name ?? "Kairos"
        let short = name.count > 28 ? String(name.prefix(27)) + "…" : name
        return tempo.phase == .idle ? short : "\(short) · \(clock(remaining))"
    }

    func tasks(in groupId: UUID?, done: Bool = false) -> [TaskItem] {
        data.tasks.filter { $0.groupId == groupId && ($0.status == .done) == done && $0.parentId == nil }
    }

    func subtasks(of id: UUID) -> [TaskItem] { data.tasks.filter { $0.parentId == id } }

    func focusMinutes(on day: Date, groupId: UUID? = nil) -> Double {
        let cal = Calendar.current
        return data.sessions.filter {
            $0.kind == .focus && cal.isDate($0.start, inSameDayAs: day) && (groupId == nil || $0.groupId == groupId)
        }.reduce(0) { $0 + $1.minutes }
    }

    // MARK: Tasks

    @discardableResult
    func addTask(from line: String) -> TaskItem? {
        let parsed = QuickAdd.parse(line, groups: data.groups.map(\.name))
        guard !parsed.title.isEmpty else { return nil }
        let group = parsed.groupName.flatMap { n in data.groups.first { $0.name.caseInsensitiveCompare(n) == .orderedSame } }
        var task = TaskItem(title: parsed.title, groupId: group?.id ?? data.focus.activeGroupId)
        task.due = parsed.due
        task.estimateMin = parsed.estimateMin
        task.priority = parsed.priority
        data.tasks.append(task)
        return task
    }

    func addSubtask(_ title: String, to parent: TaskItem) {
        let t = title.trimmingCharacters(in: .whitespaces)
        guard !t.isEmpty else { return }
        var task = TaskItem(title: t, groupId: parent.groupId)
        task.parentId = parent.id
        data.tasks.append(task)
    }

    func update(_ task: TaskItem) {
        if let i = data.tasks.firstIndex(where: { $0.id == task.id }) { data.tasks[i] = task }
    }

    func setDone(_ task: TaskItem, _ done: Bool) {
        guard let i = data.tasks.firstIndex(where: { $0.id == task.id }) else { return }
        data.tasks[i].status = done ? .done : .todo
        data.tasks[i].completedAt = done ? Date() : nil
        if done, data.focus.currentTaskId == task.id {
            data.focus.currentTaskId = nil
            suggestion = SmartNext.suggest(tasks: data.tasks, groupId: data.focus.activeGroupId, excluding: task.id,
                                           availableMin: tempo.phase == .focus ? Int(remaining / 60) : nil)
        }
    }

    func delete(_ task: TaskItem) {
        data.tasks.removeAll { $0.id == task.id || $0.parentId == task.id }
        if data.focus.currentTaskId == task.id { data.focus.currentTaskId = nil }
    }

    /// Makes a task current and turns focus on. The timer is started separately.
    func makeCurrent(_ task: TaskItem) {
        suggestion = nil
        if let g = task.groupId, g != data.focus.activeGroupId { switchGroup(g) }
        data.focus.currentTaskId = task.id
        if data.focus.mode == .off { setMode(.focus) }
    }

    // MARK: Groups and mode

    func switchGroup(_ id: UUID) {
        guard id != data.focus.activeGroupId, let next = data.group(id) else { return }
        let previous = data.activeGroup
        var focus = data.focus
        focus.activeGroupId = id
        if let task = data.currentTask, task.groupId != id { focus.currentTaskId = nil }
        data.focus = focus
        tempo.use(data.preset(named: next.tempoPreset))
        if next.desktop > 0 { AppDelegate.shared?.switchDesktop(next.desktop) }

        guard data.settings.switchBehavior == .hide else { return }
        let apps = NSWorkspace.shared.runningApplications.filter { $0.activationPolicy == .regular }
        let visible = apps.filter { !$0.isHidden }.compactMap(\.bundleIdentifier)
        let plan = GroupSwitch.plan(visible: visible, keep: Set(next.dockApps + ["com.apple.finder", Bundle.main.bundleIdentifier ?? ""]),
                                    restore: hiddenBySwitch.removeValue(forKey: id) ?? [])
        for bid in plan.unhide { apps.first { $0.bundleIdentifier == bid }?.unhide() }
        let hidden = plan.hide.filter { bid in apps.first { $0.bundleIdentifier == bid }?.hide() == true }
        if let previous { hiddenBySwitch[previous.id] = hidden }
    }

    func switchGroup(index: Int) {
        guard data.groups.indices.contains(index) else { return }
        switchGroup(data.groups[index].id)
    }

    func setMode(_ mode: Mode) {
        guard mode != data.focus.mode else { return }
        data.focus.mode = mode
    }

    /// Runs on every change of mode, whoever made it: the menu, the timer or launch.
    private func modeChanged(from old: Mode, to new: Mode) {
        // Entering Strict also clears grants, so an "Allow 5 min" made in Focus cannot carry over.
        if (new.blocks && !old.blocks) || (new == .strict && old != .strict) { ledger.reset() }
        if !new.blocks, case .app = notice?.kind { notice = nil }
    }

    @discardableResult
    func addGroup(name: String = "New group") -> UUID {
        let color = GroupPalette.colors[data.groups.count % GroupPalette.colors.count]
        let icon = GroupPalette.icons.first { i in !data.groups.contains { $0.icon == i } } ?? "square.grid.2x2"
        let group = AppGroup(name: name, icon: icon, colorHex: color)
        data.groups.append(group)
        return group.id
    }

    func renameGroup(_ id: UUID, to name: String) {
        let n = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !n.isEmpty, let i = data.groups.firstIndex(where: { $0.id == id }) else { return }
        data.groups[i].name = n
    }

    func deleteGroup(_ id: UUID) {
        guard data.groups.count > 1 else { return }
        data.groups.removeAll { $0.id == id }
        for i in data.tasks.indices where data.tasks[i].groupId == id { data.tasks[i].groupId = nil }
        if data.focus.activeGroupId == id { data.focus.activeGroupId = data.groups.first?.id }
        if selectedGroupId == id { selectedGroupId = data.focus.activeGroupId }
    }

    /// A binding to one group found by id, so a view showing a group that gets deleted reads a
    /// stale copy instead of indexing past the end of the array.
    func groupBinding(_ id: UUID) -> Binding<AppGroup> {
        let fallback = data.group(id) ?? AppGroup(name: "")
        return Binding(
            get: { self.data.group(id) ?? fallback },
            set: { g in if let i = self.data.groups.firstIndex(where: { $0.id == id }) { self.data.groups[i] = g } })
    }

    func addDockApp(_ bundleId: String, to groupId: UUID? = nil) {
        guard let i = data.groups.firstIndex(where: { $0.id == (groupId ?? data.focus.activeGroupId) }),
              !data.groups[i].dockApps.contains(bundleId) else { return }
        data.groups[i].dockApps.append(bundleId)
    }

    func removeDockApp(_ bundleId: String) {
        guard let i = data.groups.firstIndex(where: { $0.id == data.focus.activeGroupId }) else { return }
        data.groups[i].dockApps.removeAll { $0 == bundleId }
    }

    // MARK: Tempo

    /// Strict chosen by hand before the timer starts holds until the timer stops.
    func startFocus() {
        tempo.use(data.preset(named: activeGroup?.tempoPreset ?? "Classic"))
        apply(tempo.start(.focus, strict: mode == .strict, now: Date()))
    }

    func toggleTimer() {
        let t = Date()
        if tempo.phase == .idle { startFocus() }
        else if tempo.isPaused { tempo.resume(now: t); syncFocusFromTempo() }
        else { tempo.pause(now: t); syncFocusFromTempo() }
    }

    /// A strict focus phase ends only when its time runs out: Skip and Stop do nothing in it.
    var strictLocked: Bool { tempo.phase == .focus && mode == .strict }

    func skipPhase() { if !strictLocked { apply(tempo.skip(now: Date())) } }
    func stopTimer() { if !strictLocked { apply(tempo.stop(now: Date())) } }
    func addFiveMinutes() { tempo.extend(minutes: 5, now: Date()); syncFocusFromTempo() }

    private func tick() {
        now = Date()
        let events = tempo.tick(now: now)
        if !events.isEmpty { apply(events) }
        // Keeps `openSession` fresh, so a crash loses at most a minute of the running phase.
        if tempo.phase != .idle, now.timeIntervalSince(lastSave) >= 60 { saveNow() }
    }

    private func apply(_ events: [TempoEvent]) {
        for event in events {
            switch event {
            case let .finished(phase, start, end, _, completed):
                guard end.timeIntervalSince(start) >= Session.minimumLength else { continue }
                data.log(event)
                if phase == .focus, completed {
                    notify("Focus session done", body: currentTask.map { "Done with \u{201C}\($0.title)\u{201D}? Time for a break." } ?? "Time for a break.")
                } else if phase.isBreak, completed {
                    notify("Break over", body: currentTask.map { "Back to \u{201C}\($0.title)\u{201D}." } ?? "Ready for the next session.")
                }
            case let .started(phase):
                if phase == .focus {
                    ledger.reset()
                    if notice?.kind == .rest { notice = nil }
                } else if data.settings.breakOverlay {
                    notice = BlockNotice(kind: .rest, title: phase.label, detail: "Step away from the screen.")
                }
            case .stopped:
                notice = nil
                data.focus.mode = .off
            }
        }
        syncFocusFromTempo()
    }

    /// Mirrors the timer into FocusState, which is what the extensions and the guard follow.
    private func syncFocusFromTempo() {
        var focus = data.focus
        if tempo.phase != .idle { focus.mode = tempo.mode }
        focus.endsAt = tempo.isRunning ? tempo.endsAt : nil
        if focus != data.focus { data.focus = focus } else { objectWillChange.send() }
    }

    private func notify(_ title: String, body: String) {
        let content = UNMutableNotificationContent()
        content.title = title
        content.body = body
        content.sound = .default
        UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil))
    }

    // MARK: App guard

    private static let neverGuard: Set<String> = [
        "com.apple.finder", "com.apple.dock", "com.apple.systempreferences", "com.apple.loginwindow",
        "com.apple.Spotlight", "com.apple.controlcenter", "com.apple.notificationcenterui", "com.apple.SecurityAgent",
        "com.apple.Terminal", "com.apple.ActivityMonitor",
    ]

    private func appActivated(_ app: NSRunningApplication) {
        guard mode.blocks, let group = activeGroup, let bid = app.bundleIdentifier,
              bid != Bundle.main.bundleIdentifier, !Self.neverGuard.contains(bid),
              !ledger.isAllowed(bid, now: Date()) else { return }
        let name = app.localizedName ?? bid
        if group.blockedApps.contains(bid) {
            block(app, name: name, layer: "app", detail: "\(name) is blocked in \(group.name).")
            return
        }
        // Jev judges apps that are on no list. Apps in the group's dock are trusted.
        guard group.jevEnabled, hasJevKey, !group.dockApps.contains(bid), let task = currentTask else { return }
        var subject = JevClient.Subject(kind: "app")
        subject.app = name
        subject.title = Apps.frontWindowTitle(pid: app.processIdentifier) ?? ""
        let key = Keychain.get("typesafe")
        let threshold = group.jevThreshold
        Task {
            guard let verdict = try? await jev.check(apiKey: key, task: task.title, group: group.name,
                                                     groupId: group.id.uuidString, subject: subject) else { return }
            // Only act if the window Jev judged is still in front and we are still blocking.
            guard verdict.shouldBlock(threshold: threshold), self.mode.blocks,
                  NSWorkspace.shared.frontmostApplication?.bundleIdentifier == bid,
                  (Apps.frontWindowTitle(pid: app.processIdentifier) ?? "") == subject.title,
                  !self.ledger.isAllowed(bid, now: Date()) else { return }
            self.block(app, name: name, layer: "jev", detail: "Jev thinks \(name) is a distraction from your task.")
        }
    }

    private func block(_ app: NSRunningApplication, name: String, layer: String, detail: String) {
        guard let bid = app.bundleIdentifier else { return }
        let strict = mode == .strict
        let screen = Apps.displayShowing(pid: app.processIdentifier) // before its windows go
        if strict { app.terminate() } else { app.hide() }
        data.logBlock(BlockEvent(groupId: data.focus.activeGroupId, target: bid, label: name, layer: layer))
        notice = BlockNotice(kind: .app(bundleId: bid), title: strict ? "\(name) was quit" : "\(name) is hidden",
                             detail: detail, screen: screen)
    }

    /// Whether the overlay offers "Allow": read live, so it goes as soon as overrides run out.
    var noticeCanOverride: Bool {
        guard case .app = notice?.kind else { return false }
        return mode == .focus && overridesLeft > 0
    }

    /// "Allow 5 min" from the overlay. Returns an error message when refused.
    func overrideNotice(reason: String) -> String? {
        guard let notice, case let .app(bundleId) = notice.kind else { return nil }
        switch requestOverride(target: bundleId, reason: reason) {
        case .success:
            self.notice = nil
            Apps.open(bundleId)
            return nil
        case let .failure(refusal): return refusal.rawValue
        }
    }

    private func requestOverride(target: String, reason: String) -> Result<Date, OverrideLedger.Refusal> {
        let result = ledger.request(target: target, reason: reason, mode: mode, max: data.settings.maxOverridesPerSession,
                                    minutes: data.settings.overrideMinutes, now: Date())
        if case .success = result {
            data.markOverridden(target: target)
            pushState()
        }
        return result
    }

    func dismissNotice() {
        if notice?.kind == .rest { skipPhase() }
        notice = nil
    }

    // MARK: Bridge

    private func pushState(force: Bool = false) {
        let message: [String: Any] = ["type": "state", "state": BridgeState.payload(data: data, ledger: ledger, cloudVoice: cloudVoiceReady)]
        guard let encoded = try? JSONSerialization.data(withJSONObject: message, options: .sortedKeys) else { return }
        guard force || encoded != lastPushed else { return }
        lastPushed = encoded
        bridge.broadcast(message)
    }

    func sendReadCommand(_ command: String) {
        bridge.broadcast(["type": "read.command", "command": command], to: "read")
    }

    private func handle(_ m: [String: Any], from client: BridgeServer.ClientID) {
        guard let type = m["type"] as? String else { return }
        let id = m["id"] as? String
        func str(_ k: String) -> String { m[k] as? String ?? "" }
        func reply(_ payload: [String: Any]) {
            var p = payload
            if let id { p["id"] = id }
            bridge.send(p, to: client)
        }
        let stateMessage: [String: Any] = ["type": "state", "state": BridgeState.payload(data: data, ledger: ledger, cloudVoice: cloudVoiceReady)]

        switch type {
        case "hello":
            bridge.setName(str("client"), for: client)
            reply(stateMessage)
        case "getState":
            reply(stateMessage)
        case "app.show":
            // A second launch of Kairos on this profile hands over to this one and quits.
            AppDelegate.shared?.showMainWindow()
        case "blockEvent":
            data.logBlock(BlockEvent(groupId: data.focus.activeGroupId, target: Rules.hostOf(urlString: str("target")),
                                     layer: str("layer"), overridden: m["overridden"] as? Bool ?? false))
        case "override.request":
            switch requestOverride(target: Rules.normalizeSite(str("target")), reason: str("reason")) {
            case let .success(until):
                reply(["type": "override.result", "allowed": true, "until": Int(until.timeIntervalSince1970 * 1000), "remaining": overridesLeft])
            case let .failure(refusal):
                reply(["type": "override.result", "allowed": false, "remaining": overridesLeft, "message": refusal.rawValue])
            }
        case "rule.addElement":
            let host = Rules.normalizeSite(str("host")), selector = str("selector")
            guard !host.isEmpty, !selector.isEmpty else { return }
            data.updateActiveGroup { $0.hideElement(selector, on: host) }
        case "rule.removeElement":
            let host = Rules.normalizeSite(str("host")), selector = str("selector")
            guard !host.isEmpty, !selector.isEmpty else { return }
            data.updateActiveGroup { $0.showElement(selector, on: host) }
        case "read.status":
            let state = str("state")
            readStatus = state == "stopped" ? nil : ReadStatus(state: state, title: str("title"))
        case "read.log":
            data.logListening(seconds: (m["seconds"] as? NSNumber)?.intValue ?? 0)
        case "tts.speak":
            speak(text: str("text"), rate: (m["rate"] as? NSNumber)?.floatValue ?? 1, reply: reply)
        case "tts.prefetch":
            let text = str("text"), settings = data.settings
            guard cloudVoiceReady, !text.isEmpty, let account = settings.voiceProvider.keyAccount else { return }
            let key = Keychain.get(account), voices = self.voices
            Task { _ = try? await voices.synthesize(provider: settings.voiceProvider, apiKey: key, voiceId: settings.voiceId, text: text) }
        case "tts.stop":
            speechGeneration += 1
            speech.stop()
        case "tts.pause":
            speech.pause()
        case "tts.resume":
            speech.resume()
        case "jev.check":
            jevCheck(m, reply: reply)
        case "scene.candidates", "scene.generate", "scene.edit":
            sceneRequest(type, m, reply: reply)
        default:
            break
        }
    }

    /// Speaks one sentence with the chosen cloud voice and replies `tts.done` when it ends.
    private func speak(text: String, rate: Float, reply: @escaping ([String: Any]) -> Void) {
        let settings = data.settings
        guard cloudVoiceReady, let account = settings.voiceProvider.keyAccount else {
            return reply(["type": "error", "message": "No cloud voice is set up in Kairos Desktop."])
        }
        speech.stop()
        speechGeneration += 1
        let generation = speechGeneration
        let key = Keychain.get(account)
        Task {
            do {
                let audio = try await voices.synthesize(provider: settings.voiceProvider, apiKey: key, voiceId: settings.voiceId, text: text)
                // A stop or a newer sentence arrived while this one was being fetched.
                guard generation == self.speechGeneration else { return reply(["type": "tts.done", "stopped": true]) }
                try self.speech.play(audio, rate: rate) { stopped in reply(["type": "tts.done", "stopped": stopped]) }
                self.lastVoiceError = nil
            } catch {
                self.lastVoiceError = error.localizedDescription
                reply(["type": "error", "message": error.localizedDescription])
            }
        }
    }

    private func jevCheck(_ m: [String: Any], reply: @escaping ([String: Any]) -> Void) {
        func unsure(_ block: Bool) {
            reply(["type": "jev.result", "choice": "unsure", "confidence": 0, "probabilities": [String: Double](), "block": block, "cached": false])
        }
        let url = m["url"] as? String ?? ""
        let host = Rules.hostOf(urlString: url)
        guard mode.blocks, let group = activeGroup, group.jevEnabled, let task = currentTask,
              !Rules.host(host, in: data.settings.neverAnalyse), !ledger.isAllowed(host, now: Date()) else { return unsure(false) }
        var subject = JevClient.Subject(kind: "page")
        subject.url = url
        subject.title = m["title"] as? String ?? ""
        subject.description = m["description"] as? String ?? ""
        subject.excerpt = m["excerpt"] as? String ?? ""
        let key = Keychain.get("typesafe")
        let failClosed = data.settings.jevFailClosed
        Task {
            do {
                let v = try await jev.check(apiKey: key, task: task.title, group: group.name, groupId: group.id.uuidString, subject: subject)
                self.lastJevError = nil
                reply(["type": "jev.result", "choice": v.choice, "confidence": v.confidence, "probabilities": v.probabilities,
                       "block": v.shouldBlock(threshold: group.jevThreshold), "cached": v.cached])
            } catch {
                self.lastJevError = error.localizedDescription
                // No key means Jev is simply not set up: never block on that.
                if case JevError.noKey = error { unsure(false) } else { unsure(failClosed) }
            }
        }
    }

    /// Chrome drops the port on a host message over 1 MB, so an oversized scene becomes an error.
    nonisolated static func sceneReply(_ html: String) -> [String: Any] {
        guard html.utf8.count <= 900_000 else {
            return ["type": "error", "message": "That scene came out too large to send to Chrome (over 900 KB). Try a simpler subject."]
        }
        return ["type": "scene.result", "html": html]
    }

    private func sceneRequest(_ type: String, _ m: [String: Any], reply: @escaping ([String: Any]) -> Void) {
        let key = Keychain.get("anthropic")
        let title = m["title"] as? String ?? "", text = m["text"] as? String ?? ""
        let subject = m["subject"] as? String ?? "", html = m["html"] as? String ?? ""
        let instruction = m["instruction"] as? String ?? ""
        let scenes = self.scenes
        Task {
            do {
                switch type {
                case "scene.candidates":
                    let list = try await scenes.candidates(apiKey: key, title: title, text: text)
                    reply(["type": "scene.candidates.result",
                           "candidates": list.map { ["name": $0.name, "kind": $0.kind, "why": $0.why] }])
                case "scene.generate":
                    reply(Self.sceneReply(try await scenes.generate(apiKey: key, title: title, text: text, subject: subject)))
                default:
                    reply(Self.sceneReply(try await scenes.edit(apiKey: key, html: html, instruction: instruction)))
                }
            } catch {
                reply(["type": "error", "message": error.localizedDescription])
            }
        }
    }
}
