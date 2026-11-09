import Foundation

/// Turns termination signals into a call on `queue`, so Kairos can put the macOS Dock back and
/// save before it exits. Without it `kill`, Ctrl-C in a terminal or a closed terminal window
/// end Kairos at once and `applicationWillTerminate` never runs.
public final class SignalTrap {
    public static let termination: [Int32] = [SIGTERM, SIGINT, SIGHUP]

    private let signals: [Int32]
    private var sources: [DispatchSourceSignal] = []

    public init(_ signals: [Int32] = SignalTrap.termination, queue: DispatchQueue = .main, handler: @escaping (Int32) -> Void) {
        self.signals = signals
        for number in signals {
            // The default action would end the process before the source sees the signal.
            signal(number, SIG_IGN)
            let source = DispatchSource.makeSignalSource(signal: number, queue: queue)
            source.setEventHandler { handler(number) }
            source.resume()
            sources.append(source)
        }
    }

    /// Stops listening and gives the signals their default action back.
    public func cancel() {
        sources.forEach { $0.cancel() }
        sources = []
        for number in signals { signal(number, SIG_DFL) }
    }

    deinit { cancel() }
}
