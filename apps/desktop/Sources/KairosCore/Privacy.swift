import Foundation

/// `Settings.neverAnalyse` covers every path that sends something from a page off the Mac:
/// Jev, cloud voices and scenes (PROTOCOL.md, "Privacy").
public enum Privacy {
    /// Bridge requests whose text goes to a cloud service. Kairos Read passes the page's `url`.
    public static let contentRequests: Set<String> = ["tts.speak", "tts.prefetch", "scene.candidates", "scene.generate", "scene.edit"]

    /// The error message to answer a request with when it carries text from a listed page, or nil
    /// when it may go. A request without a `url` cannot be checked here; Kairos Read checks it too.
    public static func refusal(_ message: [String: Any], neverAnalyse: [String]) -> String? {
        guard let type = message["type"] as? String, contentRequests.contains(type),
              let url = message["url"] as? String, !url.isEmpty else { return nil }
        let host = Rules.hostOf(urlString: url)
        guard !host.isEmpty, Rules.host(host, in: neverAnalyse) else { return nil }
        return "\(host) is on your \u{201C}Never send off this Mac\u{201D} list in Kairos, so nothing from it goes to a cloud service."
    }

    /// What Jev is told about an app. A browser's window title is the title of its page, which can
    /// name a listed site or what is on it, so a browser is sent by name only.
    public static func appSubject(name: String, bundleId: String, windowTitle: String) -> JevClient.Subject {
        var subject = JevClient.Subject(kind: "app")
        subject.app = name
        subject.title = Browsers.isBrowser(bundleId) ? "" : windowTitle
        return subject
    }
}
