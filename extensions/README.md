# Kairos Chrome extensions

Two Manifest V3 extensions, plain ES modules with no build step. Both talk to Kairos Desktop through
the native messaging host `com.kairos.host` (see `packages/shared/PROTOCOL.md`).

| Folder | Extension | Fixed ID |
| --- | --- | --- |
| `shield/` | Kairos Shield: site rules, element hiding, Jev overlay | `jicllkgbnjlcddcddjldjnapfaigmjoi` |
| `read/` | Kairos Read: article to speech, 3D scenes | `npkcajmchbpcjigdehpiijkhkpedddcd` |

## Load

Open `chrome://extensions` (or `brave://extensions`), turn on Developer mode, choose "Load unpacked"
and pick `extensions/shield`, then again for `extensions/read`. The IDs above come from the `key` in
each manifest, so they are the same on every machine.

## Test

```sh
(cd extensions/shield && node --test)
(cd extensions/read && node --test)
```

## Shared and vendored files

- `lib/bridge.js` and `lib/match.js` are copies of `packages/shared/*.js`. Edit the originals, then
  run `scripts/sync-shared.sh`.
- `read/vendor/` holds Mozilla Readability 0.6.0 and three.js 0.170.0 (minified module build plus
  OrbitControls). Refresh with `scripts/vendor.sh`.

## Notes

- Speech uses `chrome.tts` in the service worker, one sentence per utterance. While reading, the
  worker pings an extension API every 20 seconds so Chrome does not suspend it.
- When Kairos Desktop offers a cloud voice (`state.cloudVoice`), the voice picker lists it and
  sentences go to the app as `tts.speak` instead (speed 0.5× to 2×). If the app goes away or a
  sentence fails, reading carries on from that sentence with the default Mac voice.
  `read/lib/player.js` holds the one state machine both voices run under.
- `test/harness.js` in each extension runs `background.js` under node against a fake `chrome`.
- Scenes run in `read/sandbox.html`, a manifest sandbox page with no extension APIs. The side panel
  hands it the packaged three.js sources; the page turns them into `blob:` URLs and rewrites the
  scene's import map, so scenes work offline. Its CSP is `default-src 'none'`: scene code cannot fetch,
  open sockets, load remote scripts or images, or navigate the frame away.
- A scene build runs in Read's service worker, not the side panel: closing the panel does not lose
  it. The finished scene is saved against its article (newest 20 kept) and shown when the panel is
  reopened; "Open in tab" shows it full size (`read/scene.html`). Cancel only stops waiting, since
  Kairos Desktop has no way to abandon a build.
- When Kairos Desktop is not running, Shield keeps the last known mode (with a manual switch in the
  popup) and Read still reads; 3D scenes and Jev need the app.
