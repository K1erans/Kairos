# Kairos bridge protocol

One JSON object per message. Between Chrome and the native host (`com.kairos.host`) Chrome's
native-messaging framing applies (4-byte little-endian length + UTF-8 JSON). The host is a dumb
relay to Kairos Desktop over a Unix socket (newline-delimited JSON). Every message has a `type`.
Requests that expect an answer carry a string `id`, echoed in the reply. A failed request is
answered with `{type:"error", id, message}`.

Extension IDs (fixed by the `key` in each manifest):

- Kairos Shield: `jicllkgbnjlcddcddjldjnapfaigmjoi`
- Kairos Read: `npkcajmchbpcjigdehpiijkhkpedddcd`

## Extension -> app

| type | fields | reply |
| --- | --- | --- |
| `hello` | `client` ("shield" or "read"), `version` | `state` |
| `getState` | | `state` |
| `jev.check` | `id`, `url`, `title`, `description`, `excerpt` | `jev.result` |
| `blockEvent` | `target` (host or URL), `layer` ("rule" or "jev"), `overridden` (bool) | none |
| `override.request` | `id`, `target` (hostname), `reason` | `override.result` |
| `rule.addElement` | `host`, `selector` | `state` (pushed) |
| `rule.removeElement` | `host`, `selector` | `state` (pushed) |
| `rule.addSite` | `site` (hostname), `list` ("block" or "allow") | `state` (pushed) |
| `read.status` | `state` ("playing", "paused", "stopped"), `title`, `url` | none |
| `read.log` | `seconds`, `url`, `title` | none |
| `tts.speak` | `id`, `text` (one sentence), `rate` (0.5 to 2; clamped), `url` (page being read) | `tts.done` when the audio ends |
| `tts.prefetch` | `text` (the next sentence), `url` | none |
| `tts.pause`, `tts.resume`, `tts.stop` | | none (`tts.stop` makes a pending `tts.speak` answer with `stopped: true`) |
| `scene.candidates` | `id`, `title`, `text`, `url` | `scene.candidates.result` |
| `scene.generate` | `id`, `title`, `text`, `subject`, `url` | `scene.result` |
| `scene.edit` | `id`, `html`, `instruction` | `scene.result` |
| `scene.cancel` | `id` (of the `scene.*` request to stop) | none; that request is answered with `error {id, message}` |

## App -> extension

`state` (pushed on every change and in reply to `hello` / `getState`):

```json
{
  "type": "state",
  "state": {
    "mode": "off | focus | break | strict",
    "activeGroupId": "uuid",
    "groupName": "Work",
    "groupColor": "#4F7CFF",
    "currentTask": { "id": "uuid", "title": "Write report" },
    "endsAt": 1794252486430,
    "overridesLeft": 3,
    "overrideMinutes": 5,
    "strictUntil": 1794252486430,
    "cloudVoice": { "provider": "elevenlabs", "name": "ElevenLabs" },
    "rules": {
      "blockSites": ["youtube.com"],
      "allowSites": ["docs.google.com"],
      "hiddenElements": { "youtube.com": ["#related"] },
      "jevEnabled": true,
      "jevThreshold": 0.7,
      "neverAnalyse": ["mail.google.com"],
      "temporaryAllows": { "reddit.com": 1794252786430 }
    }
  }
}
```

`currentTask`, `endsAt` (ms since epoch) and `cloudVoice` may be `null`. `cloudVoice` is set when a
cloud voice (ElevenLabs or OpenAI) is chosen in Kairos Desktop and its key is saved. Kairos Desktop
fetches and plays that audio itself: the extension sends one `tts.speak` per sentence and waits for
`tts.done`. A site matches a rule when its hostname
equals the entry or ends with `.` + entry. `temporaryAllows` maps hostname -> expiry (ms); an entry
that has not expired beats block rules and Jev.

Which layers apply per mode:

| mode | site rules | element rules | Jev |
| --- | --- | --- | --- |
| off | no | no | no |
| focus | yes | yes | yes |
| break | no | yes | no |
| strict | yes | yes | yes (no overrides) |

Other pushes:

- `jev.result`: `id`, `choice` ("relevant", "distracting", "unsure"), `confidence`,
  `probabilities` (object), `block` (bool, already thresholded by the app), `cached` (bool)
- `override.result`: `id`, `allowed` (bool), `until` (ms, when allowed), `remaining` (int),
  `message` (why not, when refused)
- `tts.done`: `id`, `stopped` (bool; true when it was cut short by `tts.stop` or a newer `tts.speak`)
- `read.command`: `command` ("toggle", "stop", "next", "prev")
- `scene.candidates.result`: `id`, `candidates` (array of `{name, kind, why}`, at most 3)
- `scene.result`: `id`, `html` (a complete standalone HTML document)
- `error`: `id`, `message`

## Limits and conventions

- `rule.addSite` adds the site to the active group's block or allow list and removes it from the
  other list. "Block this site" in Shield's popup and "Always allow" on a Jev overlay send it.
- While a strict focus phase is locked (`strictUntil` set) rules can only get stricter: the app
  refuses `rule.addSite` with `list: "allow"` and `rule.removeElement`, and pushes `state` so the
  extension's copy matches again. `rule.addSite` with `list: "block"` and `rule.addElement` still
  work. Kairos Shield refuses both in its own UI first.
- `overrideMinutes` is how long one override lasts; the extensions show it ("Allow 5 min").
- `strictUntil` (ms, or `null`) is set while a strict focus phase is locked: the time it ends, or,
  when the timer is paused, the time it would end if resumed now. While the last state an
  extension saw has `strictUntil` in the future, the extension keeps enforcing strict even if the
  app disconnects, and refuses any manual mode change in its own UI until that time passes.
- Privacy: `rules.neverAnalyse` applies to every path that sends page content off the Mac. Kairos
  Read must not send `tts.speak`, `tts.prefetch` or `scene.*` for a page whose host is on the
  list (it reads such pages with the Mac voice), and it passes `url` so the app can refuse too:
  the app answers `error {id, message}` for a listed host.

- `scene.cancel` stops a running `scene.candidates`, `scene.generate` or `scene.edit`: Kairos
  Desktop terminates the scene helper (Claude subscription) or drops the HTTP request (API key),
  and answers the original request with `error {id, message: "The scene build was cancelled."}`.
  An unknown or finished `id` is ignored. A requester that wants to cancel later picks the
  request's `id` itself.
- Chrome rejects a message from the host that is larger than 1 MB and drops the port. Kairos
  Desktop must answer a scene that would exceed about 900 KB with `error {id, message}` instead.
- Kairos Read re-sends `read.status` after every reconnect, so the app's play/pause control is
  never stale.
- Kairos Read pauses a cloud voice with `tts.stop` and speaks the sentence again on resume, the
  same as for Mac voices. `tts.pause` and `tts.resume` exist on the app side but are unused.

## Scene contract

A scene is one HTML file. It imports three.js through an import map with exactly these two entries,
which Kairos Read rewrites to its packaged copy when showing the scene in the side panel:

```html
<script type="importmap">
{"imports":{"three":"https://unpkg.com/three@0.170.0/build/three.module.js",
"three/addons/":"https://unpkg.com/three@0.170.0/examples/jsm/"}}
</script>
```

Only `three` and `three/addons/controls/OrbitControls.js` may be imported. For narration sync the
scene listens for `window.addEventListener("message", e => ...)` with
`e.data = {type:"kairos:sentence", text}` and highlights the part the sentence mentions.
