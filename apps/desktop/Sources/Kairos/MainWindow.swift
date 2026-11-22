import AppKit
import ApplicationServices
import Charts
import KairosCore
import SwiftUI
import UniformTypeIdentifiers

enum Page: String, CaseIterable, Identifiable {
    case today = "Today", tasks = "Tasks", groups = "Groups", rules = "Rules", stats = "Stats", settings = "Settings"
    var id: String { rawValue }
    var icon: String {
        switch self {
        case .today: return "sun.max"
        case .tasks: return "checklist"
        case .groups: return "square.grid.2x2"
        case .rules: return "hand.raised"
        case .stats: return "chart.bar"
        case .settings: return "gearshape"
        }
    }
}

struct MainWindow: View {
    @EnvironmentObject var model: AppModel
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        NavigationSplitView {
            List(Page.allCases, selection: $model.page) { p in
                Label(p.rawValue, systemImage: p.icon).tag(p)
            }
            .navigationSplitViewColumnWidth(min: 160, ideal: 180)
        } detail: {
            Group {
                switch model.page ?? .today {
                case .today: TodayPage()
                case .tasks: TasksPage()
                case .groups: GroupsPage()
                case .rules: RulesPage()
                case .stats: StatsPage()
                case .settings: SettingsPage()
                }
            }
            .navigationTitle((model.page ?? .today).rawValue)
        }
        .onAppear { AppDelegate.shared?.openMainWindow = { openWindow(id: "main") } }
        .sheet(isPresented: Binding(get: { !model.data.onboarded }, set: { if !$0 { model.data.onboarded = true } })) {
            OnboardingSheet()
        }
    }
}

// MARK: - Shared pieces

struct GroupPicker: View {
    @EnvironmentObject var model: AppModel
    @Binding var selection: UUID?

    var body: some View {
        Picker("Group", selection: $selection) {
            ForEach(model.data.groups) { g in
                Label(g.name, systemImage: g.icon).tag(Optional(g.id))
            }
        }
        .pickerStyle(.segmented)
        .labelsHidden()
        .fixedSize()
    }
}

struct TaskRow: View {
    @EnvironmentObject var model: AppModel
    let task: TaskItem
    var showGroup = false

    var body: some View {
        HStack(spacing: 10) {
            Button {
                model.setDone(task, task.status != .done)
            } label: {
                Image(systemName: task.status == .done ? "checkmark.circle.fill" : "circle").font(.title3)
            }
            .buttonStyle(.plain)
            .accessibilityLabel(task.status == .done ? "Mark not done" : "Mark done")

            VStack(alignment: .leading, spacing: 2) {
                Text(task.title)
                    .strikethrough(task.status == .done)
                    .foregroundStyle(task.status == .done ? .secondary : .primary)
                HStack(spacing: 8) {
                    if showGroup, let g = model.data.group(task.groupId) {
                        Label(g.name, systemImage: g.icon).foregroundStyle(Color(hex: g.colorHex))
                    }
                    if let due = task.due {
                        Label(DueText.short(due, now: model.now), systemImage: "calendar")
                            .foregroundStyle(due < Date() && task.status == .todo ? .red : .secondary)
                            .help(due.formatted(date: .complete, time: .shortened))
                    }
                    if let e = task.estimateMin { Label("\(e) min", systemImage: "clock") }
                    if task.priority > 0 { Label(["", "Low", "Medium", "High"][task.priority], systemImage: "flag.fill") }
                    let subs = model.subtasks(of: task.id)
                    if !subs.isEmpty { Label("\(subs.filter { $0.status == .done }.count)/\(subs.count)", systemImage: "list.bullet") }
                }
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .fixedSize(horizontal: false, vertical: true)
            }
            Spacer()
            if model.data.focus.currentTaskId == task.id {
                Text("Current").font(.caption.weight(.semibold))
                    .padding(.horizontal, 8).padding(.vertical, 3)
                    .background(Color.accentColor.opacity(0.18), in: Capsule())
            } else if task.status == .todo {
                Button(model.tempo.phase == .idle ? "Start" : "Work on this") { model.startTask(task) }.controlSize(.small)
                    .help(model.tempo.phase == .idle ? "Make this the current task and start a focus session" : "Make this the current task")
            }
        }
        .padding(.vertical, 3)
        .contextMenu {
            Button("Delete", role: .destructive) { model.delete(task) }
        }
    }
}

/// Something that is not working the way the owner probably thinks it is.
struct WarningBanner<Action: View>: View {
    let systemImage: String
    let text: String
    @ViewBuilder var action: () -> Action

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Image(systemName: systemImage).foregroundStyle(.orange)
            Text(text).fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 8)
            action()
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.orange.opacity(0.14), in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Color.orange.opacity(0.35)))
    }
}

struct QuickAddField: View {
    @EnvironmentObject var model: AppModel
    @State private var text = ""

    var body: some View {
        HStack {
            Image(systemName: "plus.circle.fill").foregroundStyle(.secondary)
            TextField("Add a task, like \u{201C}Report tomorrow 3pm\u{201D}", text: $text)
                .textFieldStyle(.plain)
                .help("Type a title, then any of: a day and time (tomorrow 3pm), @Group, ~45m for an estimate, !high for priority.")
                .onSubmit { if model.addTask(from: text) != nil { text = "" } }
        }
        .padding(10)
        .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 10))
    }
}

struct StringListEditor: View {
    let title: String
    let placeholder: String
    @Binding var items: [String]
    var normalize: (String) -> String = { $0.trimmingCharacters(in: .whitespaces) }
    /// False while a strict focus phase locks the list against loosening.
    var canAdd = true
    var canRemove = true
    @State private var draft = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title).font(.headline)
            ForEach(items, id: \.self) { item in
                HStack {
                    Text(item).textSelection(.enabled)
                    Spacer()
                    if canRemove {
                        Button { items.removeAll { $0 == item } } label: { Image(systemName: "minus.circle") }
                            .buttonStyle(.plain).foregroundStyle(.secondary)
                            .accessibilityLabel("Remove \(item)")
                    }
                }
            }
            HStack {
                TextField(placeholder, text: $draft).textFieldStyle(.roundedBorder).onSubmit(add)
                Button("Add", action: add).disabled(normalize(draft).isEmpty)
            }
            .disabled(!canAdd)
        }
    }

    private func add() {
        let v = normalize(draft)
        guard canAdd, !v.isEmpty else { return }
        if !items.contains(v) { items.append(v) }
        draft = ""
    }
}

struct AppListEditor: View {
    let title: String
    let empty: String
    @Binding var bundleIds: [String]
    /// Up and down buttons on each row, for the order of apps on Kairos Bar.
    var reorderable = false
    /// False while a strict focus phase locks the list against loosening.
    var canRemove = true
    @State private var picking = false

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text(title).font(.headline)
                Spacer()
                Button("Add apps…") { picking = true }
            }
            if bundleIds.isEmpty { Text(empty).foregroundStyle(.secondary).font(.callout) }
            ForEach(bundleIds, id: \.self) { id in
                HStack {
                    Image(nsImage: Apps.icon(id)).resizable().frame(width: 22, height: 22)
                    Text(Apps.name(id))
                    Spacer()
                    if reorderable {
                        Button { bundleIds = bundleIds.moving(id, by: -1) } label: { Image(systemName: "chevron.up") }
                            .buttonStyle(.plain).foregroundStyle(.secondary)
                            .disabled(bundleIds.first == id)
                            .accessibilityLabel("Move \(Apps.name(id)) up")
                        Button { bundleIds = bundleIds.moving(id, by: 1) } label: { Image(systemName: "chevron.down") }
                            .buttonStyle(.plain).foregroundStyle(.secondary)
                            .disabled(bundleIds.last == id)
                            .accessibilityLabel("Move \(Apps.name(id)) down")
                    }
                    if canRemove {
                        Button { bundleIds.removeAll { $0 == id } } label: { Image(systemName: "minus.circle") }
                            .buttonStyle(.plain).foregroundStyle(.secondary)
                            .accessibilityLabel("Remove \(Apps.name(id))")
                    }
                }
            }
        }
        .sheet(isPresented: $picking) { AppPicker(selection: $bundleIds) }
    }
}

struct AppPicker: View {
    @Binding var selection: [String]
    @Environment(\.dismiss) private var dismiss
    @State private var apps: [InstalledApp] = []
    @State private var query = ""

    private var matches: [InstalledApp] { apps.filter { query.isEmpty || $0.name.localizedCaseInsensitiveContains(query) } }

    private func picked(_ app: InstalledApp) -> Binding<Bool> {
        Binding(get: { selection.contains(app.bundleId) }, set: { on in
            if on { if !selection.contains(app.bundleId) { selection.append(app.bundleId) } }
            else { selection.removeAll { $0 == app.bundleId } }
        })
    }

    var body: some View {
        VStack(spacing: 0) {
            TextField("Search apps", text: $query).textFieldStyle(.roundedBorder).padding(12)
                // Return ticks or unticks the first match, so the keyboard alone can pick apps.
                .onSubmit { if let first = matches.first { picked(first).wrappedValue.toggle() } }
            List(matches) { app in
                Toggle(isOn: picked(app)) {
                    HStack {
                        Image(nsImage: NSWorkspace.shared.icon(forFile: app.url.path)).resizable().frame(width: 26, height: 26)
                        Text(app.name)
                    }
                }
                .toggleStyle(.checkbox)
            }
            HStack { Spacer(); Button("Done") { dismiss() }.keyboardShortcut(.defaultAction) }.padding(12)
        }
        .frame(width: 420, height: 520)
        .task { apps = InstalledApp.scan() }
    }
}

// MARK: - Today

/// The timer on Today: a large countdown while a phase runs, "Break over" while the timer waits
/// for the next focus session, and the group and mode when idle.
struct TimerReadout: View {
    @EnvironmentObject var model: AppModel

    var body: some View {
        if model.tempo.phase != .idle {
            VStack(alignment: .leading, spacing: 4) {
                HStack(alignment: .firstTextBaseline, spacing: 10) {
                    Text(clock(model.remaining))
                        .font(.system(size: 44, weight: .semibold, design: .rounded).monospacedDigit())
                        .accessibilityLabel("\(model.tempo.phase.label), \(clock(model.remaining)) left")
                    Text(model.tempo.isPaused ? "\(model.tempo.phase.label), paused" : model.tempo.phase.label)
                        .font(.title3).foregroundStyle(.secondary)
                }
                ProgressView(value: model.tempo.progress(now: model.now))
                    .tint(model.tempo.phase == .focus ? .accentColor : .green)
                    .frame(maxWidth: 320)
            }
        } else if model.tempo.awaitingFocus {
            Label("Break over. Start the next focus session when you\u{2019}re ready.", systemImage: "cup.and.saucer.fill")
                .font(.title3)
        } else {
            Text("\(model.activeGroup?.name ?? "") · \(model.mode.label)").foregroundStyle(.secondary)
        }
    }
}

struct TodayPage: View {
    @EnvironmentObject var model: AppModel

    private var todays: [TaskItem] {
        let cal = Calendar.current
        return model.data.tasks.filter { t in
            guard t.status == .todo, t.parentId == nil else { return false }
            if t.id == model.data.focus.currentTaskId { return true }
            guard let due = t.due else { return false }
            return cal.isDateInToday(due) || due < Date()
        }
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                HStack(alignment: .top, spacing: 16) {
                    card {
                        VStack(alignment: .leading, spacing: 8) {
                            Text("Current task").font(.caption).foregroundStyle(.secondary)
                            Text(model.currentTask?.title ?? "Nothing yet").font(.title2.weight(.semibold))
                            TimerReadout()
                            HStack {
                                Button(model.tempo.phase == .idle ? "Start focus" : (model.tempo.isPaused ? "Resume" : "Pause")) { model.toggleTimer() }
                                    .buttonStyle(.borderedProminent)
                                if model.tempo.awaitingFocus {
                                    Button("Not now") { model.endBreakWait() }
                                        .help("Stop waiting. The next focus session starts a new count toward the long break.")
                                }
                                if model.tempo.phase != .idle {
                                    Button("Skip") { model.requestSkip() }.disabled(model.strictLocked)
                                    Button("+5 min") { model.addFiveMinutes() }
                                    Button("Stop") { model.stopTimer() }.disabled(model.strictLocked)
                                }
                                if let t = model.currentTask { Button("Done") { model.setDone(t, true) } }
                            }
                            ModePicker()
                                .pickerStyle(.segmented)
                                .fixedSize()
                                .help(model.tempo.phase == .idle ? "Off, Focus, Break or Strict, without a timer"
                                                                 : "The timer sets the mode. Stop it to change the mode by hand.")
                            if let next = model.suggestion {
                                Button { model.makeCurrent(next) } label: { Label("Next up: \(next.title)", systemImage: "sparkles") }
                                    .buttonStyle(.link)
                            }
                        }
                    }
                    card {
                        VStack(alignment: .leading, spacing: 8) {
                            Text("Today").font(.caption).foregroundStyle(.secondary)
                            Text("\(Int(model.focusMinutes(on: Date()))) min").font(.title2.weight(.semibold))
                            Text("focused").foregroundStyle(.secondary)
                            let blocked = model.data.blockEvents.filter { Calendar.current.isDateInToday($0.time) }.count
                            Text("\(blocked) blocked attempt\(blocked == 1 ? "" : "s")").foregroundStyle(.secondary)
                        }
                    }
                    .frame(width: 200)
                }
                if model.siteBlockingOff {
                    WarningBanner(systemImage: "exclamationmark.shield.fill", text: AppModel.siteBlockingOffText) {
                        Button("Chrome bridge settings") { model.page = .settings }
                    }
                }
                if model.mode.blocks, let problem = model.jevReadiness(for: model.activeGroup).problem {
                    WarningBanner(systemImage: "exclamationmark.triangle.fill", text: problem) { EmptyView() }
                }
                if !model.data.setupHidden {
                    SetupCard()
                }
                HStack {
                    Text("Group").font(.headline)
                    GroupPicker(selection: Binding(get: { model.data.focus.activeGroupId }, set: { if let id = $0 { model.switchGroup(id) } }))
                }
                QuickAddField()
                Text("Due today").font(.headline)
                if todays.isEmpty {
                    Text("Nothing due today. Add a task above, or press ⌥Space from anywhere.").foregroundStyle(.secondary)
                }
                ForEach(todays) { TaskRow(task: $0, showGroup: true) }
            }
            .padding(20)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func card<C: View>(@ViewBuilder _ content: () -> C) -> some View {
        content()
            .padding(16)
            .frame(maxWidth: .infinity, minHeight: 130, alignment: .topLeading)
            .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 14))
    }
}

/// The setup list on Today, until everything is done or the owner hides it.
struct SetupCard: View {
    @EnvironmentObject var model: AppModel
    @State private var accessibility = AXIsProcessTrusted()
    @State private var openAtLogin = LoginItem.enabled

    var body: some View {
        let list = model.setupChecklist(accessibility: accessibility, openAtLogin: openAtLogin)
        Group {
            if !list.isComplete {
                VStack(alignment: .leading, spacing: 10) {
                    HStack {
                        Text("Set up Kairos").font(.headline)
                        Text("\(list.doneCount) of \(list.items.count) done").foregroundStyle(.secondary).monospacedDigit()
                        Spacer()
                        Button("Hide") { model.data.setupHidden = true }
                            .help("Everything here is also in Settings.")
                    }
                    ForEach(list.items) { item in row(item) }
                }
                .padding(16)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 14))
            }
        }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
            accessibility = AXIsProcessTrusted()
            openAtLogin = LoginItem.enabled
        }
    }

    private func row(_ item: SetupChecklist.Item) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Image(systemName: item.done ? "checkmark.circle.fill" : "circle")
                .foregroundStyle(item.done ? Color.green : Color.secondary)
                .accessibilityLabel(item.done ? "Done" : "Not done")
            VStack(alignment: .leading, spacing: 2) {
                Text(item.title).strikethrough(item.done, color: .secondary).foregroundStyle(item.done ? .secondary : .primary)
                if !item.done { Text(item.detail).font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true) }
            }
            Spacer(minLength: 8)
            action(item)
        }
    }

    @ViewBuilder private func action(_ item: SetupChecklist.Item) -> some View {
        switch item.step {
        case .shield, .read:
            if !item.done, let folder = BridgeInstaller.extensionsFolder {
                Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting([folder]) }
            }
        case .jevKey, .voiceKey:
            if !item.done { Button("Add key…") { model.page = .settings } }
        case .scenes:
            if !item.done { Button(model.data.settings.sceneBackend == .apiKey ? "Add key…" : "Settings…") { model.page = .settings } }
        case .accessibility:
            if !item.done {
                Button("Allow…") { _ = AXIsProcessTrustedWithOptions(["AXTrustedCheckOptionPrompt": true] as CFDictionary) }
            }
        case .openAtLogin:
            Toggle("", isOn: Binding(get: { openAtLogin }, set: { LoginItem.set($0); openAtLogin = LoginItem.enabled }))
                .labelsHidden().toggleStyle(.switch).controlSize(.small)
        case .dockReplacement:
            Toggle("", isOn: Binding(get: { model.data.settings.replacesSystemDock },
                                     set: { model.data.settings.setReplacesSystemDock($0) }))
                .labelsHidden().toggleStyle(.switch).controlSize(.small)
        }
    }
}

// MARK: - Tasks

struct TasksPage: View {
    @EnvironmentObject var model: AppModel
    @State private var groupId: UUID?
    @State var selected: UUID?
    @State private var showDone = false

    var body: some View {
        HSplitView {
            VStack(alignment: .leading, spacing: 12) {
                HStack {
                    GroupPicker(selection: $groupId)
                    Spacer()
                    Toggle("Show done", isOn: $showDone).toggleStyle(.checkbox)
                }
                QuickAddField()
                List(selection: $selected) {
                    ForEach(model.tasks(in: groupId)) { TaskRow(task: $0).tag($0.id) }
                    if showDone {
                        Section("Done") { ForEach(model.tasks(in: groupId, done: true)) { TaskRow(task: $0).tag($0.id) } }
                    }
                }
                .listStyle(.inset)
            }
            .padding(16)
            .frame(minWidth: 380)

            if let task = model.data.tasks.first(where: { $0.id == selected }) {
                TaskDetail(task: task).frame(minWidth: 260, idealWidth: 300)
            } else {
                Text("Select a task").foregroundStyle(.secondary).frame(minWidth: 260, maxHeight: .infinity)
            }
        }
        .onAppear { if groupId == nil { groupId = model.data.focus.activeGroupId } }
    }
}

struct TaskDetail: View {
    @EnvironmentObject var model: AppModel
    let task: TaskItem
    @State private var subtask = ""

    private func bind<T>(_ path: WritableKeyPath<TaskItem, T>) -> Binding<T> {
        Binding(get: { task[keyPath: path] }, set: { var t = task; t[keyPath: path] = $0; model.update(t) })
    }

    var body: some View {
        Form {
            TextField("Title", text: bind(\.title))
            Picker("Group", selection: bind(\.groupId)) {
                ForEach(model.data.groups) { Text($0.name).tag(Optional($0.id)) }
            }
            Picker("Priority", selection: bind(\.priority)) {
                Text("None").tag(0); Text("Low").tag(1); Text("Medium").tag(2); Text("High").tag(3)
            }
            Toggle("Has a due date", isOn: Binding(get: { task.due != nil }, set: { var t = task; t.due = $0 ? Date() : nil; model.update(t) }))
            if task.due != nil {
                DatePicker("Due", selection: Binding(get: { task.due ?? Date() }, set: { var t = task; t.due = $0; model.update(t) }))
            }
            LabeledContent("Estimate (min)") {
                TextField("Minutes", value: bind(\.estimateMin), format: .number)
                    .labelsHidden().multilineTextAlignment(.trailing).frame(width: 90)
            }
            Section("Notes") { TextEditor(text: bind(\.notes)).frame(minHeight: 70) }
            Section("Subtasks") {
                ForEach(model.subtasks(of: task.id)) { sub in
                    Toggle(sub.title, isOn: Binding(get: { sub.status == .done }, set: { model.setDone(sub, $0) }))
                        .toggleStyle(.checkbox)
                }
                TextField("Add a subtask", text: $subtask).onSubmit { model.addSubtask(subtask, to: task); subtask = "" }
            }
            if task.listenedSec > 0 {
                LabeledContent("Listened with Kairos Read", value: "\(task.listenedSec / 60) min")
            }
        }
        .formStyle(.grouped)
    }
}

// MARK: - Groups

struct GroupSidebar: View {
    @EnvironmentObject var model: AppModel
    var allowEdit = false
    @State private var deleting: AppGroup?

    var body: some View {
        VStack(spacing: 0) {
            List(selection: $model.selectedGroupId) {
                ForEach(model.data.groups) { g in
                    Label { Text(g.name) } icon: { Image(systemName: g.icon).foregroundStyle(Color(hex: g.colorHex)) }
                        .tag(Optional(g.id))
                        .contextMenu {
                            if allowEdit {
                                Button("Rename") { model.selectedGroupId = g.id; model.renameGroupId = g.id }
                                Button("Delete\u{2026}") { deleting = g }.disabled(model.data.groups.count < 2)
                            }
                        }
                }
                .onMove { model.data.groups.move(fromOffsets: $0, toOffset: $1) }
            }
            .listStyle(.sidebar)
            if allowEdit {
                Divider()
                Button {
                    let id = model.addGroup()
                    model.selectedGroupId = id
                    model.renameGroupId = id
                } label: {
                    Label("New Group", systemImage: "plus")
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.borderless)
                .keyboardShortcut("n", modifiers: [.command, .shift])
                .help("Add a group (\u{21E7}\u{2318}N)")
                .padding(.horizontal, 12).padding(.vertical, 10)
            }
        }
        .frame(width: 190)
        .onAppear {
            if model.data.group(model.selectedGroupId) == nil { model.selectedGroupId = model.data.focus.activeGroupId }
        }
        .confirmDelete($deleting, model: model)
    }
}

extension View {
    /// Asks before deleting a group. Tasks in it are kept, with no group.
    func confirmDelete(_ group: Binding<AppGroup?>, model: AppModel) -> some View {
        confirmationDialog("Delete \u{201C}\(group.wrappedValue?.name ?? "")\u{201D}?",
                           isPresented: Binding(get: { group.wrappedValue != nil }, set: { if !$0 { group.wrappedValue = nil } }),
                           presenting: group.wrappedValue) { g in
            Button("Delete", role: .destructive) { model.deleteGroup(g.id) }
        } message: { _ in
            Text("Its apps and blocking rules are removed. Its tasks are kept, with no group.")
        }
    }
}

struct GroupsPage: View {
    @EnvironmentObject var model: AppModel

    var body: some View {
        HStack(spacing: 0) {
            GroupSidebar(allowEdit: true)
            Divider()
            if let id = model.selectedGroupId, model.data.group(id) != nil {
                GroupDetail(id: id).id(id)
            } else {
                Text("Select a group").foregroundStyle(.secondary).frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
    }
}

struct GroupDetail: View {
    @EnvironmentObject var model: AppModel
    let id: UUID
    @SwiftUI.FocusState private var nameFocused: Bool
    @State private var deleting: AppGroup?

    var body: some View {
        let g = model.groupBinding(id)
        let index = model.data.groups.firstIndex { $0.id == id } ?? 0
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                Form {
                    TextField("Name", text: g.name).focused($nameFocused)
                    Picker("Timer preset", selection: g.tempoPreset) {
                        ForEach(model.data.settings.presets) { Text("\($0.name) (\($0.focusMin)/\($0.shortBreakMin))").tag($0.name) }
                    }
                    LabeledContent("Shortcut", value: index < 9 && model.data.settings.groupHotkeys ? "\u{2303}\(index + 1)" : "None")
                    Picker("Switch to desktop", selection: g.desktop) {
                        Text("Don't switch").tag(0)
                        ForEach(1...9, id: \.self) { Text("Desktop \($0)").tag($0) }
                    }
                    if g.wrappedValue.desktop > 0 {
                        Text("Needs the Accessibility permission (Settings > Blocking) and \u{201C}Switch to Desktop \(g.wrappedValue.desktop)\u{201D} turned on in System Settings > Keyboard > Keyboard Shortcuts > Mission Control.")
                            .font(.callout).foregroundStyle(.secondary)
                    }
                }
                VStack(alignment: .leading, spacing: 6) {
                    Text("Colour").font(.headline)
                    HStack {
                        ForEach(Array(GroupPalette.colors.enumerated()), id: \.element) { i, hex in
                            let chosen = g.wrappedValue.colorHex == hex
                            Button { g.wrappedValue.colorHex = hex } label: {
                                Circle().fill(Color(hex: hex)).frame(width: 22, height: 22)
                                    .overlay(Circle().strokeBorder(.primary, lineWidth: chosen ? 2 : 0))
                            }
                            .buttonStyle(.plain)
                            .help(GroupPalette.colorNames[i])
                            .accessibilityLabel(GroupPalette.colorNames[i])
                            .accessibilityAddTraits(chosen ? .isSelected : [])
                        }
                    }
                    Text("Icon").font(.headline).padding(.top, 6)
                    HStack {
                        ForEach(GroupPalette.icons, id: \.self) { name in
                            let chosen = g.wrappedValue.icon == name
                            Button { g.wrappedValue.icon = name } label: {
                                Image(systemName: name).frame(width: 28, height: 28)
                                    .background(chosen ? Color.accentColor.opacity(0.25) : .clear, in: RoundedRectangle(cornerRadius: 6))
                                    .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .help(GroupPalette.iconName(name))
                            .accessibilityLabel(GroupPalette.iconName(name))
                            .accessibilityAddTraits(chosen ? .isSelected : [])
                        }
                    }
                }
                AppListEditor(title: "Apps in Kairos Bar", empty: "No apps yet. Add some, or drag an app onto Kairos Bar.",
                              bundleIds: g.dockApps, reorderable: true)
                Divider()
                Button("Delete Group\u{2026}", role: .destructive) { deleting = g.wrappedValue }
                    .disabled(model.data.groups.count < 2)
            }
            .padding(20)
        }
        .confirmDelete($deleting, model: model)
        // Kept until the field takes focus, because the detail can be rebuilt before it is in a window.
        .task(id: model.renameGroupId) {
            guard model.renameGroupId == id else { return }
            try? await Task.sleep(for: .milliseconds(50))
            nameFocused = true
        }
        .onChange(of: nameFocused) { _, focused in
            if focused, model.renameGroupId == id { model.renameGroupId = nil }
        }
    }
}

// MARK: - Rules

struct RulesPage: View {
    @EnvironmentObject var model: AppModel

    var body: some View {
        HStack(spacing: 0) {
            GroupSidebar()
            Divider()
            if let id = model.selectedGroupId, model.data.group(id) != nil {
                let g = model.groupBinding(id)
                let lock = model.ruleLock(for: id)
                ScrollView {
                    VStack(alignment: .leading, spacing: 22) {
                        Text("These apply while \(g.wrappedValue.name) is the active group and the mode is Focus or Strict.")
                            .foregroundStyle(.secondary)
                        if let lock {
                            Label(lock, systemImage: "lock.fill").foregroundStyle(.orange)
                        }
                        AppListEditor(title: "Blocked apps", empty: "No apps blocked. Add other browsers here to stop site rules being bypassed.",
                                      bundleIds: g.blockedApps, canRemove: lock == nil)
                        StringListEditor(title: "Blocked sites", placeholder: "example.com", items: g.blockSites, normalize: Rules.normalizeSite,
                                         canRemove: lock == nil)
                        StringListEditor(title: "Always allowed sites", placeholder: "docs.google.com", items: g.allowSites, normalize: Rules.normalizeSite,
                                         canAdd: lock == nil)
                        hidden(g, locked: lock != nil)
                        VStack(alignment: .leading, spacing: 6) {
                            Text("Smart blocking with Jev").font(.headline)
                            Text("Jev is a service from TypeSafe that checks whether a page or app fits your current task. It needs a TypeSafe key (Settings > API keys).")
                                .font(.callout).foregroundStyle(.secondary)
                            Toggle("Ask Jev about sites and apps that are on no list", isOn: g.jevEnabled)
                                .disabled(lock != nil && g.wrappedValue.jevEnabled)
                            HStack {
                                Text("Block when \u{201C}distracting\u{201D} is at least")
                                Slider(value: g.jevThreshold, in: 0.5...0.95, step: 0.05).frame(width: 160)
                                Text(g.wrappedValue.jevThreshold.formatted(.number.precision(.fractionLength(2)))).monospacedDigit()
                            }
                            .disabled(!g.wrappedValue.jevEnabled || lock != nil)
                            if let problem = model.jevReadiness(for: g.wrappedValue).problem {
                                Label(problem, systemImage: "exclamationmark.triangle.fill")
                                    .font(.callout).foregroundStyle(.orange)
                            }
                        }
                    }
                    .padding(20)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            } else {
                Text("Select a group").foregroundStyle(.secondary).frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
    }

    private func hidden(_ g: Binding<AppGroup>, locked: Bool) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("Hidden page elements").font(.headline)
            let entries = g.wrappedValue.hiddenElements.sorted { $0.key < $1.key }
            if entries.isEmpty {
                Text("None yet. Use \u{201C}Hide an element on this page\u{201D} in the Kairos Shield popup in Chrome.").foregroundStyle(.secondary).font(.callout)
            }
            ForEach(entries, id: \.key) { host, selectors in
                ForEach(selectors, id: \.self) { selector in
                    HStack {
                        Text(host).fontWeight(.medium)
                        Text(selector).font(.system(.callout, design: .monospaced)).foregroundStyle(.secondary).lineLimit(1)
                        Spacer()
                        if !locked {
                            Button {
                                g.wrappedValue.hiddenElements[host]?.removeAll { $0 == selector }
                                if g.wrappedValue.hiddenElements[host]?.isEmpty == true { g.wrappedValue.hiddenElements[host] = nil }
                            } label: { Image(systemName: "minus.circle") }
                                .buttonStyle(.plain).foregroundStyle(.secondary)
                                .accessibilityLabel("Remove rule for \(host)")
                        }
                    }
                }
            }
        }
    }
}

// MARK: - Stats

struct StatsPage: View {
    @EnvironmentObject var model: AppModel

    private struct DayBar: Identifiable {
        let id = UUID()
        let day: Date, group: String, minutes: Double
    }

    private var days: [Date] {
        let cal = Calendar.current
        let today = cal.startOfDay(for: Date())
        return (0..<7).reversed().compactMap { cal.date(byAdding: .day, value: -$0, to: today) }
    }

    private var bars: [DayBar] {
        days.flatMap { day in
            model.data.groups.map { DayBar(day: day, group: $0.name, minutes: model.focusMinutes(on: day, groupId: $0.id)) }
        }
    }

    private var weekSessions: [Session] {
        guard let first = days.first else { return [] }
        return model.data.sessions.filter { $0.kind == .focus && $0.start >= first }
    }

    private var topBlocked: [(target: String, count: Int, overridden: Int)] {
        guard let first = days.first else { return [] }
        let events = model.data.blockEvents.filter { $0.time >= first }
        return Dictionary(grouping: events, by: \.displayName)
            .map { ($0.key, $0.value.count, $0.value.filter(\.overridden).count) }
            .sorted { $0.1 > $1.1 }.prefix(8).map { $0 }
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                Text("Last 7 days").font(.headline)
                HStack(spacing: 16) {
                    stat("\(Int(weekSessions.reduce(0) { $0 + $1.minutes }))", "minutes focused")
                    stat("\(weekSessions.filter(\.completed).count)", "sessions completed")
                    stat("\(weekSessions.reduce(0) { $0 + $1.interruptions })", "interruptions")
                    stat("\(streak)", "day streak")
                }
                .fixedSize(horizontal: false, vertical: true)
                if model.data.unassignedListenedSec >= 60 {
                    Text("Listened with Kairos Read without a current task: \(model.data.unassignedListenedSec / 60) min")
                        .font(.callout).foregroundStyle(.secondary)
                }
                Text("Focus minutes by day").font(.headline)
                Chart(bars) { bar in
                    BarMark(x: .value("Day", bar.day, unit: .day), y: .value("Minutes", bar.minutes))
                        .foregroundStyle(by: .value("Group", bar.group))
                }
                .chartForegroundStyleScale(domain: model.data.groups.map(\.name), range: model.data.groups.map { Color(hex: $0.colorHex) })
                .chartXAxis { AxisMarks(values: .stride(by: .day)) { _ in AxisValueLabel(format: .dateTime.weekday(.abbreviated)) } }
                .frame(height: 220)

                Text("Most blocked").font(.headline)
                if topBlocked.isEmpty { Text("Nothing has been blocked yet.").foregroundStyle(.secondary) }
                ForEach(topBlocked, id: \.target) { row in
                    HStack {
                        Text(row.target)
                        Spacer()
                        Text("\(row.count) blocked\(row.overridden > 0 ? ", \(row.overridden) allowed" : "")").foregroundStyle(.secondary).monospacedDigit()
                    }
                }
            }
            .padding(20)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private var streak: Int {
        let cal = Calendar.current
        var day = cal.startOfDay(for: Date()), count = 0
        // Today only counts once it has a session; a missing today does not break the streak.
        if model.focusMinutes(on: day) == 0 { day = cal.date(byAdding: .day, value: -1, to: day) ?? day }
        while model.focusMinutes(on: day) > 0, count < 3650 {
            count += 1
            day = cal.date(byAdding: .day, value: -1, to: day) ?? day
        }
        return count
    }

    private func stat(_ value: String, _ label: String) -> some View {
        VStack(alignment: .leading) {
            Text(value).font(.title.weight(.semibold)).monospacedDigit()
            Text(label).font(.caption).foregroundStyle(.secondary).lineLimit(1)
        }
        .padding(14)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 12))
    }
}

// MARK: - Settings

struct SettingsPage: View {
    @EnvironmentObject var model: AppModel
    @State private var jevKey = ""
    @State private var claudeKey = ""
    @State private var elevenKey = ""
    @State private var openaiKey = ""
    @State private var loginItem = LoginItem.enabled
    @State private var browsers = BridgeInstaller.installedBrowsers()
    @State private var axTrusted = AXIsProcessTrusted()

    var body: some View {
        Form {
            Section("General") {
                Toggle("Open Kairos at login", isOn: Binding(get: { loginItem }, set: { LoginItem.set($0); loginItem = LoginItem.enabled }))
                Toggle("Notify me when a task is due", isOn: $model.data.settings.notifiesWhenDue)
                Picker("When switching group", selection: $model.data.settings.switchBehavior) {
                    ForEach(SwitchBehavior.allCases, id: \.self) { Text($0.label).tag($0) }
                }
                if model.data.settings.switchBehavior == .hide {
                    Text("Switching groups hides other apps’ windows and keeps them off Kairos Bar, then brings back the windows Kairos hid for apps in this group. Apps you open while in a group still show on Kairos Bar. Finder stays available.")
                        .font(.caption).foregroundStyle(.secondary)
                } else if model.data.settings.switchBehavior == .hideBlocked {
                    Text("Switching to a group hides its blocked apps, even with the timer off. Choose them under Rules > Blocked apps. Kairos restores their windows when you switch to a group that allows them.")
                        .font(.caption).foregroundStyle(.secondary)
                }
                Picker("Quick add", selection: $model.data.settings.quickAddHotkey) {
                    ForEach(Hotkeys.quickAdd) { Text($0.label).tag($0.id) }
                    Text("Off").tag(Hotkeys.off)
                }
                Picker("Start or pause the timer", selection: $model.data.settings.timerHotkey) {
                    ForEach(Hotkeys.timer) { Text($0.label).tag($0.id) }
                    Text("Off").tag(Hotkeys.off)
                }
                VStack(alignment: .leading, spacing: 4) {
                    Toggle("⌃1 to ⌃9 switch groups", isOn: $model.data.settings.groupHotkeys)
                    Text("macOS can use these to switch desktops (Keyboard Shortcuts > Mission Control). While this is on, Kairos takes them.")
                        .font(.callout).foregroundStyle(.secondary)
                }
                if let failure = HotkeyPlan.failureText(model.hotkeyFailures) {
                    Label(failure, systemImage: "exclamationmark.triangle.fill").foregroundStyle(.orange)
                }
            }
            Section("Kairos Bar") {
                Toggle("Show Kairos Bar", isOn: $model.data.settings.dockVisible)
                Picker("Screen edge", selection: $model.data.settings.dockEdge) {
                    ForEach(DockEdge.allCases, id: \.self) { Text($0.rawValue.capitalized).tag($0) }
                }
                Picker("Appearance", selection: $model.data.settings.barAppearance) {
                    ForEach(BarAppearance.allCases, id: \.self) { Text($0.label).tag($0) }
                }
                Slider(value: $model.data.settings.iconSize, in: 20...64, step: 2) { Text("Icon size: \(Int(model.data.settings.iconSize))") }
                Text("You can also drag the divider on Kairos Bar itself.").font(.callout).foregroundStyle(.secondary)
                Toggle("Automatically hide and show Kairos Bar", isOn: $model.data.settings.autoHideBar)
                VStack(alignment: .leading, spacing: 4) {
                    Toggle("Hide Kairos Bar in full-screen apps", isOn: model.hidesInFullScreenBinding)
                        .disabled(model.data.settings.autoHideBar)
                    Text(model.data.settings.autoHideBar
                         ? "With auto-hide on, Kairos Bar always keeps out of full-screen apps, as the Dock does."
                         : "Kairos Bar stays on every desktop and keeps out of full-screen apps. Push the pointer against the screen edge twice to bring it out there.")
                        .font(.callout).foregroundStyle(.secondary)
                }
                VStack(alignment: .leading, spacing: 4) {
                    Toggle("Keep windows clear of Kairos Bar", isOn: $model.data.settings.keepsWindowsClear)
                    Text(windowsClearNote).font(.callout).foregroundStyle(axTrusted ? .secondary : Color.orange)
                    if !axTrusted, model.data.settings.keepsWindowsClear {
                        Button("Allow Accessibility…") {
                            _ = AXIsProcessTrustedWithOptions(["AXTrustedCheckOptionPrompt": true] as CFDictionary)
                        }
                    }
                }
                Toggle("Show Downloads and Trash at the end of Kairos Bar", isOn: $model.data.settings.showsDockFolders)
                VStack(alignment: .leading, spacing: 4) {
                    Toggle("Use Kairos Bar instead of the macOS Dock", isOn: Binding(
                        get: { model.data.settings.replacesSystemDock },
                        set: { model.data.settings.setReplacesSystemDock($0) }))
                    Text("macOS can\u{2019}t remove the Dock, so Kairos hides it and puts your Dock settings back when you turn this off or quit. Turning this on also turns on \u{201C}Automatically hide and show Kairos Bar\u{201D}, so Kairos Bar slides away like the Dock did. If Kairos ever stops without putting the Dock back, a small background helper does it within 15 seconds.")
                        .font(.callout).foregroundStyle(.secondary)
                }
            }
            Section("Timer") {
                ForEach($model.data.settings.presets) { $preset in
                    HStack {
                        Text(preset.name).frame(width: 90, alignment: .leading)
                        Stepper("Focus \(preset.focusMin)", value: $preset.focusMin, in: 5...180, step: 5)
                        Stepper("Break \(preset.shortBreakMin)", value: $preset.shortBreakMin, in: 1...60)
                        Stepper("Long \(preset.longBreakMin)", value: $preset.longBreakMin, in: 5...90, step: 5)
                        Toggle("Strict", isOn: $preset.strict)
                    }
                }
                Toggle("Show a full-screen break reminder", isOn: $model.data.settings.breakOverlay)
                VStack(alignment: .leading, spacing: 4) {
                    Toggle("Start the next focus session automatically", isOn: $model.data.settings.startsFocusAfterBreak)
                    Text("Off, the timer waits when a break ends and notifies you. Start the next session from Today, the menu bar or the timer shortcut.")
                        .font(.callout).foregroundStyle(.secondary)
                }
            }
            Section("Blocking") {
                Stepper("Overrides per session: \(model.data.settings.maxOverridesPerSession)", value: $model.data.settings.maxOverridesPerSession, in: 0...10)
                Stepper("Each override lasts \(model.data.settings.overrideMinutes) min", value: $model.data.settings.overrideMinutes, in: 1...30)
                Toggle("Block the page when Jev is slow or offline", isOn: $model.data.settings.jevFailClosed)
                VStack(alignment: .leading, spacing: 4) {
                    Toggle("Block browsers that Kairos Shield is not running in", isOn: $model.data.settings.blockUnshieldedBrowsers)
                    Text("In Focus and Strict, Safari, Firefox and other browsers that can\u{2019}t run Kairos Shield are hidden, and so are Chromium browsers (Chrome, Brave, Arc, Edge) while no Kairos Shield is connected. Kairos can\u{2019}t tell which Chromium browser Shield is running in, so one connected Shield counts for all of them.")
                        .font(.callout).foregroundStyle(.secondary)
                }
                LabeledContent("Read window titles (Accessibility)") {
                    if axTrusted { Label("Allowed", systemImage: "checkmark.circle.fill").foregroundStyle(.green) }
                    else {
                        Button("Allow…") {
                            let opts = ["AXTrustedCheckOptionPrompt": true] as CFDictionary
                            _ = AXIsProcessTrustedWithOptions(opts)
                        }
                    }
                }
                if let err = model.lastJevError { Text("Last Jev error: \(err)").font(.callout).foregroundStyle(.orange) }
            }
            Section("API keys (stored in your Keychain)") {
                keyRow("TypeSafe (Jev)", has: model.hasJevKey, text: $jevKey, account: "typesafe")
                keyRow("Anthropic (3D scenes)", has: model.hasClaudeKey, text: $claudeKey, account: "anthropic")
            }
            Section("3D scenes (Kairos Read)") {
                Picker("Write scenes with", selection: $model.data.settings.sceneBackend) {
                    ForEach(SceneBackend.allCases, id: \.self) { Text($0.label).tag($0) }
                }
                let status = model.sceneSetup.status(model.data.settings.sceneBackend, hasClaudeKey: model.hasClaudeKey)
                Text(status.text).font(.callout).foregroundStyle(status.ready ? Color.secondary : Color.orange)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .onAppear { model.refreshSceneSetup() }
            Section("Reading voice (Kairos Read)") {
                Picker("Voice", selection: Binding(get: { model.data.settings.voiceProvider },
                                                   set: { model.data.settings.setVoiceProvider($0) })) {
                    ForEach(VoiceProvider.allCases, id: \.self) { Text($0.label).tag($0) }
                }
                if model.data.settings.voiceProvider == .elevenlabs {
                    keyRow("ElevenLabs API key", has: model.voiceKeys.contains("elevenlabs"), text: $elevenKey, account: "elevenlabs")
                } else if model.data.settings.voiceProvider == .openai {
                    keyRow("OpenAI API key", has: model.voiceKeys.contains("openai"), text: $openaiKey, account: "openai")
                }
                if model.data.settings.voiceProvider != .system {
                    Picker("Voice name", selection: $model.data.settings.voiceId) {
                        Text("Default").tag("")
                        ForEach(model.cloudVoices) { Text($0.name).tag($0.id) }
                    }
                    Text(model.cloudVoiceReady ? "Pick \u{201C}\(model.data.settings.voiceProvider.label)\u{201D} in the Kairos Read voice menu. Audio plays from this Mac, and each sentence is sent to \(model.data.settings.voiceProvider.label)."
                        : "Add the API key to use this voice.")
                        .font(.callout).foregroundStyle(.secondary)
                    if let err = model.lastVoiceError { Text(err).font(.callout).foregroundStyle(.orange) }
                }
            }
            .onChange(of: model.data.settings.voiceProvider, initial: true) { _, _ in model.loadCloudVoices() }
            .onChange(of: model.voiceKeys) { _, _ in model.loadCloudVoices() }
            Section("Chrome bridge") {
                LabeledContent("Installed for", value: browsers.isEmpty ? "No browser yet" : browsers.joined(separator: ", "))
                LabeledContent("Connected now", value: model.bridgeClients.isEmpty ? "Nothing" : model.bridgeClients.map { "Kairos \($0.capitalized)" }.joined(separator: ", "))
                HStack {
                    Button("Install bridge") { BridgeInstaller.install(); browsers = BridgeInstaller.installedBrowsers() }
                    if let folder = BridgeInstaller.extensionsFolder {
                        Button("Show extensions in Finder") { NSWorkspace.shared.activateFileViewerSelecting([folder]) }
                    }
                }
                Text("In Chrome or Brave open chrome://extensions, turn on Developer mode, choose \u{201C}Load unpacked\u{201D}, and pick the shield and read folders.")
                    .font(.callout).foregroundStyle(.secondary)
            }
            Section("Privacy") {
                StringListEditor(title: "Never send off this Mac", placeholder: "mybank.com",
                                 items: $model.data.settings.neverAnalyse, normalize: Rules.normalizeSite)
                Text("Jev never sees these sites, Kairos Read reads them with a Mac voice and makes no scenes from them. Jev is only told a browser\u{2019}s name, never its page title.")
                    .font(.callout).foregroundStyle(.secondary)
            }
            Section("Data") {
                HStack {
                    Button("Export everything as JSON…") { export() }
                    Button("Import…") { importFile() }
                        .disabled(!model.canImport)
                        .help(model.canImport ? "Replace everything with an exported file" : "Stop the timer to import.")
                }
                Text("Import replaces your groups, tasks, rules, history and settings with an exported file. Kairos saves a copy of what you have now first, beside its data file.")
                    .font(.callout).foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
            axTrusted = AXIsProcessTrusted()
            browsers = BridgeInstaller.installedBrowsers()
        }
    }

    private var windowsClearNote: String {
        if !axTrusted {
            return "Off until Kairos has the Accessibility permission: macOS keeps windows clear only of its own Dock, so Kairos needs Accessibility to move them."
        }
        return model.data.settings.autoHideBar
            ? "Only while Kairos Bar stays on screen. With auto-hide on, windows can go under it, as with the Dock."
            : "Windows that would sit under Kairos Bar are moved or shortened to end at its edge, as macOS does for the Dock. Full-screen windows are left alone."
    }

    private func keyRow(_ label: String, has: Bool, text: Binding<String>, account: String) -> some View {
        HStack {
            SecureField(label, text: text, prompt: Text(has ? "Saved — type to replace" : "Paste key"))
            Button("Save") { model.setKey(text.wrappedValue, account: account); text.wrappedValue = "" }
                .disabled(text.wrappedValue.isEmpty)
            if has { Button("Remove") { model.setKey("", account: account) } }
        }
    }

    private func importFile() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.json]
        panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        guard let raw = try? Data(contentsOf: url), let file = try? AppData.fromExport(raw) else {
            let alert = NSAlert()
            alert.messageText = "\u{201C}\(url.lastPathComponent)\u{201D} is not a Kairos export"
            alert.informativeText = "Choose a file made with \u{201C}Export everything as JSON\u{201D}. Nothing was changed."
            alert.runModal()
            return
        }
        let confirm = NSAlert()
        confirm.alertStyle = .warning
        confirm.messageText = "Replace everything in Kairos with this file?"
        confirm.informativeText = "\(file.groups.count) group\(file.groups.count == 1 ? "" : "s") and \(file.tasks.count) task\(file.tasks.count == 1 ? "" : "s") from \u{201C}\(url.lastPathComponent)\u{201D} take the place of yours. Kairos saves a copy of your current data first."
        confirm.addButton(withTitle: "Replace")
        confirm.addButton(withTitle: "Cancel")
        guard confirm.runModal() == .alertFirstButtonReturn else { return }
        let done = NSAlert()
        do {
            let backup = try model.importData(file)
            done.messageText = "Imported"
            done.informativeText = backup.map { "Your previous data is in \u{201C}\($0.lastPathComponent)\u{201D}, in \($0.deletingLastPathComponent().path)." }
                ?? "There was no saved data to back up."
            if let backup { done.addButton(withTitle: "OK"); done.addButton(withTitle: "Show in Finder")
                if done.runModal() == .alertSecondButtonReturn { NSWorkspace.shared.activateFileViewerSelecting([backup]) }
                return
            }
        } catch {
            done.messageText = "Import failed"
            done.informativeText = "Nothing was changed. \(error.localizedDescription)"
        }
        done.runModal()
    }

    private func export() {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "kairos-export.json"
        panel.allowedContentTypes = [.json]
        guard panel.runModal() == .OK, let url = panel.url, let data = model.exportJSON() else { return }
        try? data.write(to: url)
    }
}

// MARK: - First launch

struct OnboardingSheet: View {
    @EnvironmentObject var model: AppModel
    @State private var browsers = BridgeInstaller.installedBrowsers()

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Welcome to Kairos").font(.title.weight(.semibold))
            Text("Kairos shows only the apps for what you are doing, blocks the rest while you focus, and times your work.")
                .foregroundStyle(.secondary)
            step("1", "Groups", "You start with Work, Study and Personal, each with its own apps, blocked sites and timer. Switch group from Kairos Bar or the menu bar, and add one with the + on Kairos Bar.")
            step("2", "Tasks", "Press ⌥Space anywhere to add a task, like \u{201C}Draft report tomorrow 3pm\u{201D}. Pick one on Today and start a focus session.")
            step("3", "Set up", browsers.isEmpty
                ? "Today lists what is left to set up: the Kairos Shield extension (no Chromium browser was found yet), a key for Jev, which checks whether a page or app fits your current task, 3D scenes on your Claude subscription, and window titles."
                : "Today lists what is left to set up: loading the Kairos Shield and Kairos Read extensions, a key for Jev, which checks whether a page or app fits your current task, 3D scenes on your Claude subscription, and window titles. The browser link is installed for \(browsers.joined(separator: ", ")).")
            HStack {
                Spacer()
                Button("Go to Today") { model.page = .today; model.data.onboarded = true }.keyboardShortcut(.defaultAction)
            }
        }
        .padding(26)
        .frame(width: 520)
    }

    private func step(_ n: String, _ title: String, _ text: String) -> some View {
        HStack(alignment: .top, spacing: 12) {
            Text(n).font(.headline).frame(width: 26, height: 26).background(Color.accentColor.opacity(0.2), in: Circle())
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.headline)
                Text(text).foregroundStyle(.secondary)
            }
        }
    }
}
