import Foundation

/// The folders at the far end of Kairos Bar, after a separator, as on the Dock.
public enum DockFolder: String, CaseIterable, Hashable, Sendable {
    case downloads, trash

    public var name: String { self == .trash ? "Trash" : "Downloads" }
}

/// What files dropped on Kairos Bar do, by what is under the pointer: on Trash they go to the
/// Trash, on an app the documents open in that app, and apps dropped anywhere else join the
/// active group.
public enum DockDrop: Equatable, Sendable {
    case addApps([String])
    case open([URL], withApp: String)
    case moveToTrash([URL])
    case nothing

    /// `bundleId` names the app an .app bundle is, or nil for anything else.
    public static func plan(_ urls: [URL], on slot: DockGeometry.Slot?, bundleId: (URL) -> String?) -> DockDrop {
        guard !urls.isEmpty else { return .nothing }
        if slot == .folder(.trash) { return .moveToTrash(urls) }
        let apps = urls.filter { $0.pathExtension == "app" }.compactMap(bundleId)
        let documents = urls.filter { $0.pathExtension != "app" }
        switch slot {
        case let .app(id)?, let .running(id)?:
            if !documents.isEmpty { return .open(documents, withApp: id) }
        default:
            break
        }
        return apps.isEmpty ? .nothing : .addApps(apps)
    }
}

/// Apps started from Kairos Bar that are still launching, for the pulse on their icons.
public struct DockLaunches: Equatable, Sendable {
    /// An app that has not shown up by then is taken to have failed, and stops pulsing.
    public static let timeout: TimeInterval = 10
    private var started: [String: Date] = [:]

    public init() {}

    public var ids: Set<String> { Set(started.keys) }

    public mutating func start(_ bundleId: String, now: Date) { started[bundleId] = now }

    /// Forgets apps that are running now, and any that have run past `timeout`.
    public mutating func update(running: Set<String>, now: Date) {
        started = started.filter { !running.contains($0.key) && now.timeIntervalSince($0.value) < Self.timeout }
    }
}

extension Array where Element: Equatable {
    /// The array with `element` moved `offset` places, stopping at either end.
    public func moving(_ element: Element, by offset: Int) -> [Element] {
        guard let i = firstIndex(of: element) else { return self }
        var copy = self
        copy.remove(at: i)
        copy.insert(element, at: Swift.min(Swift.max(0, i + offset), copy.count))
        return copy
    }

    /// The array with `element` moved to `index`, counted in the result.
    public func moving(_ element: Element, to index: Int) -> [Element] {
        guard let i = firstIndex(of: element) else { return self }
        return moving(element, by: index - i)
    }
}

/// Dragging an icon along Kairos Bar to reorder it among its peers (the pinned apps, or the
/// groups).
public enum DockReorder {
    /// Where the dragged icon belongs: the peer whose resting centre is nearest the pointer.
    /// Peers are evenly spaced, so the answer does not flip back once the icon has moved there.
    public static func index(centers: [Double], pointer: Double) -> Int? {
        centers.indices.min { abs(centers[$0] - pointer) < abs(centers[$1] - pointer) }
    }
}
