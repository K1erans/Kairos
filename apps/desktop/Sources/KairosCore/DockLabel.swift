import Foundation

/// The name label over the item under the pointer on Kairos Bar. It is never clipped or cut
/// short: the panel is made long enough (bottom edge) or its label lane wide enough (side
/// edges) for the widest label, and a label near either end of the bar slides along it to stay
/// inside the panel and the visible screen, as the Dock's does near a screen corner.
public enum DockLabel {
    /// Space kept between a label and the panel's or the screen's edge.
    public static let margin = 4.0
    /// Gap between a magnified icon and its label on a side edge.
    public static let gap = 10.0

    /// The label's centre along the bar: over the item when it fits, otherwise moved just far
    /// enough to stay `margin` inside `bounds` (the panel's extent along the bar, cut to the
    /// visible screen). `width` is the label's extent along the bar.
    public static func center(item: Double, width: Double, within bounds: ClosedRange<Double>) -> Double {
        let lo = bounds.lowerBound + margin + width / 2
        let hi = bounds.upperBound - margin - width / 2
        guard lo <= hi else { return (bounds.lowerBound + bounds.upperBound) / 2 }
        return min(max(item, lo), hi)
    }

    /// How much a panel `length` long along the bar must grow at each end so a label `widest`
    /// long fits along it.
    public static func growth(length: Double, widest: Double) -> Double {
        max(0, (widest + 2 * margin - length) / 2).rounded(.up)
    }

    /// The label lane beside a side bar: room for the widest label after the gap.
    public static func sideLane(widest: Double) -> Double {
        (widest + gap + margin).rounded(.up)
    }
}
