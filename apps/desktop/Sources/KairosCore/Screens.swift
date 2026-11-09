import CoreGraphics

public enum Screens {
    /// The display that shows most of `window`. Window bounds from the window server are in
    /// global display space (origin top-left of the main display), the same space as
    /// `CGDisplayBounds`, so no flipping is needed.
    public static func display(showing window: CGRect, among displays: [(id: UInt32, bounds: CGRect)]) -> UInt32? {
        let scored = displays.map { d -> (UInt32, CGFloat) in
            let i = d.bounds.intersection(window)
            return (d.id, i.isNull ? 0 : i.width * i.height)
        }
        guard let best = scored.max(by: { $0.1 < $1.1 }), best.1 > 0 else { return nil }
        return best.0
    }
}
