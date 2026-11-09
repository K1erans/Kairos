import Foundation

/// A local notification at a task's due time.
public struct DueReminder: Equatable, Sendable {
    /// Marks Kairos's due reminders among its pending notifications, so a reschedule can find them.
    public static let idPrefix = "kairos-due-"
    public let id: String
    public let title: String
    public let body: String
    public let fireAt: Date
}

public enum DueReminders {
    /// macOS keeps at most 64 pending local notifications for an app and drops the rest.
    public static let limit = 64

    /// What the reminders depend on, compared before rescheduling.
    public struct Inputs: Equatable, Sendable {
        let tasks: [TaskItem]
        let groupNames: [UUID: String]
        let on: Bool

        public init(_ data: AppData) {
            tasks = data.tasks.filter { $0.status == .todo && $0.due != nil }
            groupNames = Dictionary(data.groups.map { ($0.id, $0.name) }, uniquingKeysWith: { a, _ in a })
            on = data.settings.notifiesWhenDue
        }
    }

    /// The soonest unfinished tasks due after `now`, at most `limit` of them. Later ones are
    /// scheduled as these fire and Kairos plans again.
    public static func plan(_ inputs: Inputs, now: Date) -> [DueReminder] {
        guard inputs.on else { return [] }
        let upcoming: [(task: TaskItem, due: Date)] = inputs.tasks.compactMap { task in
            guard let due = task.due, due > now else { return nil }
            return (task, due)
        }
        return upcoming
            .sorted { ($0.due, $0.task.id.uuidString) < ($1.due, $1.task.id.uuidString) }
            .prefix(limit)
            .map { entry in
                let group = entry.task.groupId.flatMap { inputs.groupNames[$0] }
                return DueReminder(id: DueReminder.idPrefix + entry.task.id.uuidString, title: entry.task.title,
                                   body: group.map { "Due now, in \($0)." } ?? "Due now.", fireAt: entry.due)
            }
    }
}
