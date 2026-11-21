//! Text to speech through ElevenLabs or OpenAI. Returns MP3 data; the app plays it.

use crate::models::VoiceProvider;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::sync::Mutex;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub struct CloudVoice {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum VoiceError {
    NoKey,
    Http(u16, String),
    Empty,
    Network(String),
}

impl std::fmt::Display for VoiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VoiceError::NoKey => f.write_str("No API key is set for the chosen voice provider."),
            VoiceError::Http(c, b) => write!(f, "The voice service returned HTTP {c}: {}", b.chars().take(200).collect::<String>()),
            VoiceError::Empty => f.write_str("The voice service returned no audio."),
            VoiceError::Network(e) => write!(f, "The voice service could not be reached: {e}"),
        }
    }
}

pub const ELEVENLABS_DEFAULT_VOICE: &str = "21m00Tcm4TlvDq8ikWAM"; // "Rachel"
pub const OPENAI_VOICES: &[&str] = &["alloy", "ash", "ballad", "coral", "echo", "fable", "nova", "onyx", "sage", "shimmer"];

/// (url, headers, body) for one sentence, or None for the built-in voices.
pub fn request(provider: VoiceProvider, api_key: &str, voice_id: &str, text: &str) -> Option<(String, Vec<(String, String)>, Value)> {
    match provider {
        VoiceProvider::System => None,
        VoiceProvider::Elevenlabs => {
            // An OpenAI name saved before the provider changed is not an ElevenLabs voice.
            let voice = if voice_id.is_empty() || OPENAI_VOICES.contains(&voice_id) { ELEVENLABS_DEFAULT_VOICE } else { voice_id };
            let safe: String = voice.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
            Some((
                format!("https://api.elevenlabs.io/v1/text-to-speech/{safe}?output_format=mp3_44100_128"),
                vec![("xi-api-key".into(), api_key.into())],
                json!({"text": text, "model_id": "eleven_flash_v2_5"}),
            ))
        }
        VoiceProvider::Openai => Some((
            "https://api.openai.com/v1/audio/speech".into(),
            vec![("Authorization".into(), format!("Bearer {api_key}"))],
            json!({"model": "gpt-4o-mini-tts", "voice": if OPENAI_VOICES.contains(&voice_id) { voice_id } else { "alloy" }, "input": text, "response_format": "mp3"}),
        )),
    }
}

#[derive(Default)]
pub struct VoiceClient {
    cache: Mutex<(HashMap<String, Vec<u8>>, VecDeque<String>)>,
}

impl VoiceClient {
    /// Synthesises `text`, reusing audio already fetched for the same provider, voice and text.
    pub fn synthesize(&self, provider: VoiceProvider, api_key: Option<&str>, voice_id: &str, text: &str) -> Result<Vec<u8>, VoiceError> {
        let key = api_key.filter(|k| !k.is_empty()).ok_or(VoiceError::NoKey)?;
        let cache_key = format!("{}|{voice_id}|{text}", provider.raw());
        if let Some(hit) = self.cache.lock().unwrap().0.get(&cache_key) {
            return Ok(hit.clone());
        }
        let (url, headers, body) = request(provider, key, voice_id, text).ok_or(VoiceError::NoKey)?;
        let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(30)).build();
        let mut req = agent.post(&url).set("Content-Type", "application/json");
        for (k, v) in &headers {
            req = req.set(k, v);
        }
        let data = match req.send_string(&body.to_string()) {
            Ok(r) => {
                let mut b = Vec::new();
                r.into_reader().take(50_000_000).read_to_end(&mut b).map_err(|e| VoiceError::Network(e.to_string()))?;
                b
            }
            Err(ureq::Error::Status(c, r)) => return Err(VoiceError::Http(c, r.into_string().unwrap_or_default())),
            Err(e) => return Err(VoiceError::Network(e.to_string())),
        };
        if data.is_empty() {
            return Err(VoiceError::Empty);
        }
        let mut cache = self.cache.lock().unwrap();
        if !cache.0.contains_key(&cache_key) {
            cache.1.push_back(cache_key.clone());
        }
        cache.0.insert(cache_key, data.clone());
        while cache.1.len() > 12 {
            if let Some(old) = cache.1.pop_front() {
                cache.0.remove(&old);
            }
        }
        Ok(data)
    }

    pub fn elevenlabs_voices(&self, api_key: &str) -> Result<Vec<CloudVoice>, VoiceError> {
        let resp = ureq::get("https://api.elevenlabs.io/v1/voices").set("xi-api-key", api_key).timeout(Duration::from_secs(20)).call();
        match resp {
            Ok(r) => Ok(parse_elevenlabs_voices(r.into_string().unwrap_or_default().as_bytes())),
            Err(ureq::Error::Status(c, r)) => Err(VoiceError::Http(c, r.into_string().unwrap_or_default())),
            Err(e) => Err(VoiceError::Network(e.to_string())),
        }
    }
}

pub fn parse_elevenlabs_voices(data: &[u8]) -> Vec<CloudVoice> {
    let Ok(root) = serde_json::from_slice::<Value>(data) else { return vec![] };
    root.get("voices")
        .and_then(|v| v.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|v| Some(CloudVoice { id: v.get("voice_id")?.as_str()?.into(), name: v.get("name")?.as_str()?.into() }))
                .collect()
        })
        .unwrap_or_default()
}
