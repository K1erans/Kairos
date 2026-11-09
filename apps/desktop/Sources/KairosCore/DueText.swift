import Foundation

/// A due date as short as a task row can show it on one line: "Today 15:00", "Tomorrow 09:00",
/// "Yesterday 15:00", "Wed 15:00" within the coming week, "20 Nov 15:00" this year, and
/// "5 Jan 2027" further off.
public enum DueText {
    public static func short(_ due: Date, now: Date, calendar: Calendar = .current, locale: Locale = .current) -> String {
        func format(_ style: Date.FormatStyle) -> String {
            var style = style
            style.locale = locale
            style.calendar = calendar
            style.timeZone = calendar.timeZone
            return due.formatted(style)
        }
        let time = format(Date.FormatStyle(date: .omitted, time: .shortened))
        let today = calendar.startOfDay(for: now)
        let days = calendar.dateComponents([.day], from: today, to: calendar.startOfDay(for: due)).day ?? 0
        switch days {
        case 0: return "Today \(time)"
        case 1: return "Tomorrow \(time)"
        case -1: return "Yesterday \(time)"
        case 2...6: return "\(format(Date.FormatStyle().weekday(.abbreviated))) \(time)"
        default:
            if calendar.component(.year, from: due) == calendar.component(.year, from: now) {
                return "\(format(Date.FormatStyle().day().month(.abbreviated))) \(time)"
            }
            return format(Date.FormatStyle().day().month(.abbreviated).year())
        }
    }
}
