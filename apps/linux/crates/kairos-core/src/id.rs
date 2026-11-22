use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use uuid::Uuid;

/// A UUID written upper-case, as Swift's `UUID.uuidString` writes it, so files and exports move
/// between the Mac and Linux apps unchanged (`taskByGroup` is keyed by that string).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Id(pub Uuid);

impl Id {
    pub fn new() -> Self {
        Id(Uuid::new_v4())
    }
    pub fn parse(s: &str) -> Option<Self> {
        Uuid::parse_str(s.trim()).ok().map(Id)
    }
    pub fn string(&self) -> String {
        self.0.hyphenated().to_string().to_uppercase()
    }
}

impl Default for Id {
    fn default() -> Self {
        Id::new()
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.string())
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.string())
    }
}

impl Serialize for Id {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.string())
    }
}

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Id::parse(&s).ok_or_else(|| serde::de::Error::custom("not a UUID"))
    }
}
