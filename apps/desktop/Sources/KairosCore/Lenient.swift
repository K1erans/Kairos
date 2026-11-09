import Foundation

/// A persisted type that decodes field by field. A field that is missing or unreadable takes
/// its value from `fallback`, so a file written by an older or newer Kairos still loads. A
/// field listed with `.required` has no sensible fallback: without it the whole value fails,
/// and inside a list only that element is dropped.
///
/// Conforming replaces the synthesized `init(from:)`; encoding stays synthesized. A test checks
/// that `fields` names every stored property, so a new field cannot be left out.
public protocol Lenient: Codable {
    /// The value a missing field falls back to. Computed, so ids and dates are fresh each time.
    static var fallback: Self { get }
    static var fields: [LenientField<Self>] { get }
}

extension Lenient {
    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: AnyCodingKey.self)
        var value = Self.fallback
        for field in Self.fields { try field.decode(container, &value) }
        self = value
    }
}

public struct LenientField<Root> {
    public let key: String
    public let required: Bool
    let decode: (KeyedDecodingContainer<AnyCodingKey>, inout Root) throws -> Void

    public static func field<V: Decodable>(_ key: String, _ path: WritableKeyPath<Root, V>) -> Self {
        Self(key: key, required: false) { c, root in
            if let v = try? c.decodeIfPresent(V.self, forKey: AnyCodingKey(key)) { root[keyPath: path] = v }
        }
    }

    /// A list of lenient values keeps every element that decodes and drops the rest.
    public static func field<E: Lenient>(_ key: String, _ path: WritableKeyPath<Root, [E]>) -> Self {
        Self(key: key, required: false) { c, root in
            guard var list = try? c.nestedUnkeyedContainer(forKey: AnyCodingKey(key)) else { return }
            var out: [E] = []
            while !list.isAtEnd {
                if let e = try? list.decode(E.self) { out.append(e) } else { _ = try? list.decode(Skip.self) }
            }
            root[keyPath: path] = out
        }
    }

    public static func required<V: Decodable>(_ key: String, _ path: WritableKeyPath<Root, V>) -> Self {
        Self(key: key, required: true) { c, root in
            root[keyPath: path] = try c.decode(V.self, forKey: AnyCodingKey(key))
        }
    }
}

public struct AnyCodingKey: CodingKey {
    public let stringValue: String
    public var intValue: Int? { nil }
    public init(_ string: String) { stringValue = string }
    public init?(stringValue: String) { self.stringValue = stringValue }
    public init?(intValue: Int) { nil }
}

/// Decodes anything, to step over a list element that failed.
private struct Skip: Decodable {
    init(from decoder: Decoder) throws {}
}
