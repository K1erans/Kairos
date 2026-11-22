//! Decodes a kairos.json (from either app), re-encodes it, and lists keys that were lost.
use serde_json::Value;
fn keys(v: &Value, path: String, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => for (k, x) in m { let p = format!("{path}.{k}"); out.push(p.clone()); keys(x, p, out) },
        Value::Array(a) => if let Some(x) = a.first() { keys(x, format!("{path}[]"), out) },
        _ => {}
    }
}
fn main() {
    let path = std::env::args().nth(1).expect("path");
    let raw = std::fs::read(&path).unwrap();
    let data = kairos_core::AppData::from_export(&raw).expect("decode");
    let back: Value = serde_json::from_slice(&kairos_core::store::encode(&data)).unwrap();
    let orig: Value = serde_json::from_slice(&raw).unwrap();
    let (mut a, mut b) = (vec![], vec![]);
    keys(&orig, String::new(), &mut a);
    keys(&back, String::new(), &mut b);
    let lost: Vec<_> = a.iter().filter(|k| !b.contains(k)).collect();
    println!("groups {} tasks {} sessions {} events {}", data.groups.len(), data.tasks.len(), data.sessions.len(), data.block_events.len());
    println!("lost keys: {lost:?}");
    let added: Vec<_> = b.iter().filter(|k| !a.contains(k)).collect();
    println!("added keys: {added:?}");
}
