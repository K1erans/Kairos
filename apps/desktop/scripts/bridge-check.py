#!/usr/bin/env python3
"""Drives a built Kairos.app through its native host, the way Chrome does, against a scratch
profile (KAIROS_HOME), and checks the bridge end to end. Never touches the real profile.

Usage: scripts/bridge-check.py [path/to/Kairos.app]
"""
import json, os, socket, struct, subprocess, sys, tempfile, threading, time, uuid

APP = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "build/Kairos.app")
HOME = tempfile.mkdtemp(prefix="kairos-check-")
SOCK = os.path.join(HOME, "bridge.sock")
DATA = os.path.join(HOME, "kairos.json")
ENV = dict(os.environ, KAIROS_HOME=HOME, KAIROS_START_FOCUS="1")
results = []


def check(name, ok, detail=""):
    results.append(ok)
    print(("PASS " if ok else "FAIL ") + name + (f"  ({detail})" if detail else ""))


def seed():
    group = str(uuid.uuid4()).upper()
    task = {"id": str(uuid.uuid4()).upper(), "title": "Write report", "notes": "", "priority": 0, "status": "todo",
            "createdAt": "2026-11-09T10:00:00Z", "listenedSec": 0, "groupId": group}
    data = {
        "groups": [{"id": group, "name": "Work", "blockSites": ["youtube.com", "reddit.com"], "jevEnabled": False}],
        "tasks": [task], "sessions": [], "blockEvents": [],
        "settings": {"dockVisible": False, "maxOverridesPerSession": 3},
        # Focus with no end time is what an older build kept after a relaunch; the fixed build
        # starts focus through KAIROS_START_FOCUS instead.
        "focus": {"activeGroupId": group, "mode": "focus"},
        "onboarded": True,
    }
    with open(DATA, "w") as f:
        json.dump(data, f)


def listening():
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        s.connect(SOCK)
        return True
    except OSError:
        return False
    finally:
        s.close()


def wait_for(cond, timeout=8.0):
    end = time.time() + timeout
    while time.time() < end:
        if cond():
            return True
        time.sleep(0.05)
    return False


class Host:
    """kairos-host speaking Chrome native-messaging framing on stdin/stdout."""

    def __init__(self):
        self.p = subprocess.Popen([os.path.join(APP, "Contents/MacOS/kairos-host")], env=ENV,
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE)
        self.frames, self.invalid = [], 0
        self.lock = threading.Lock()
        threading.Thread(target=self.read, daemon=True).start()

    def read(self):
        out = self.p.stdout
        while True:
            head = out.read(4)
            if len(head) < 4:
                return
            body = out.read(struct.unpack("<I", head)[0])
            try:
                msg = json.loads(body)
            except ValueError:
                with self.lock:
                    self.invalid += 1
                continue
            with self.lock:
                self.frames.append(msg)

    def send(self, msg):
        body = json.dumps(msg).encode()
        self.p.stdin.write(struct.pack("<I", len(body)) + body)
        self.p.stdin.flush()

    def reply(self, rid, timeout=5.0):
        found = []
        def has():
            with self.lock:
                found[:] = [m for m in self.frames if m.get("id") == rid]
            return bool(found)
        return found[0] if wait_for(has, timeout) else None

    def latest_state(self):
        with self.lock:
            states = [m for m in self.frames if m.get("type") == "state"]
        return states[-1]["state"] if states else None

    def close(self):
        self.p.stdin.close()
        self.p.wait(timeout=5)


def saved():
    time.sleep(1.2)  # the app saves 0.5 s after the last change
    with open(DATA) as f:
        return json.load(f)


def relaunch_check():
    """D5: a profile saved mid-timer comes back with blocking off and the phase logged."""
    seed()
    with open(DATA) as f:
        data = json.load(f)
    data["openSession"] = {"id": str(uuid.uuid4()).upper(), "start": "2026-11-09T10:00:00Z", "end": "2026-11-09T10:12:00Z",
                           "kind": "focus", "interruptions": 1, "completed": True, "groupId": data["groups"][0]["id"]}
    with open(DATA, "w") as f:
        json.dump(data, f)
    env = dict(ENV, KAIROS_START_FOCUS="0")
    app = subprocess.Popen([os.path.join(APP, "Contents/MacOS/Kairos")], env=env,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        wait_for(listening)
        host = Host()
        host.send({"type": "getState", "id": "boot"})
        r = host.reply("boot")
        mode = (r or {}).get("state", {}).get("mode")
        check("a profile saved mid-timer relaunches with mode off", mode == "off", f"mode {mode}")
        d = saved()
        sessions = [(x["end"], x["completed"]) for x in d.get("sessions", [])]
        check("the interrupted phase is logged up to the last save", sessions == [("2026-11-09T10:12:00Z", False)]
              and d.get("openSession") is None, json.dumps(sessions))
        host.close()
    finally:
        app.terminate()
        app.wait(timeout=5)
    wait_for(lambda: not listening(), 3)


def strict_relaunch_check():
    """A strict focus phase saved before a quit comes back strict, with its end, at launch."""
    seed()
    with open(DATA) as f:
        data = json.load(f)
    until = int(time.time()) + 20 * 60
    stamp = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(until))
    data["focus"].update({"mode": "strict", "strictUntil": stamp, "endsAt": stamp})
    data["settings"]["overrideMinutes"] = 7
    with open(DATA, "w") as f:
        json.dump(data, f)
    app = subprocess.Popen([os.path.join(APP, "Contents/MacOS/Kairos")], env=dict(ENV, KAIROS_START_FOCUS="0"),
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        wait_for(listening)
        host = Host()
        host.send({"type": "getState", "id": "strict"})
        state = (host.reply("strict") or {}).get("state", {})
        check("a strict phase saved before a quit resumes strict, with the same end",
              state.get("mode") == "strict" and state.get("strictUntil") == until * 1000,
              f"mode {state.get('mode')}, strictUntil {state.get('strictUntil')}, expected {until * 1000}")
        check("the pushed state carries overrideMinutes", state.get("overrideMinutes") == 7, f"{state.get('overrideMinutes')}")
        host.send({"type": "override.request", "id": "so", "target": "youtube.com", "reason": "please"})
        r = host.reply("so") or {}
        check("the resumed strict phase refuses overrides", r.get("allowed") is False, json.dumps(r))
        host.close()
    finally:
        app.terminate()
        app.wait(timeout=5)
    wait_for(lambda: not listening(), 3)


def main():
    relaunch_check()
    strict_relaunch_check()
    seed()
    app = subprocess.Popen([os.path.join(APP, "Contents/MacOS/Kairos")], env=ENV,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        if not wait_for(listening):
            check("app listens on the scratch socket", False)
            return
        host = Host()

        host.send({"type": "hello", "client": "shield", "version": "check", "id": "h1"})
        r = host.reply("h1")
        check("hello is answered with state echoing its id", r is not None and r.get("type") == "state")
        host.send({"type": "getState", "id": "g1"})
        r = host.reply("g1")
        check("getState is answered with state echoing its id", r is not None and r.get("type") == "state")
        mode = (host.latest_state() or {}).get("mode")
        print(f"     mode on the bridge: {mode}")

        # D1: big state pushes and replies racing to one client must arrive whole.
        pad = "x" * 40_000
        for i in range(40):
            host.send({"type": "rule.addElement", "host": "example.com", "selector": f"#s{i}{pad}"})
            host.send({"type": "getState", "id": f"race{i}"})
        got = wait_for(lambda: host.reply("race39", 0.01) is not None, 20)
        time.sleep(0.5)
        with host.lock:
            n = len(host.frames)
        check("40 large pushes and 40 replies arrive as valid JSON", got and host.invalid == 0,
              f"{n} frames, {host.invalid} invalid")

        # A1: rule.removeElement drops each selector and pushes state.
        for i in range(40):
            host.send({"type": "rule.removeElement", "host": "example.com", "selector": f"#s{i}{pad}"})
        host.send({"type": "getState", "id": "after-remove"})
        r = host.reply("after-remove", 10)
        hidden = (r or {}).get("state", {}).get("rules", {}).get("hiddenElements", {})
        check("rule.removeElement removes the selectors (host entry gone)", r is not None and "example.com" not in hidden,
              f"hiddenElements keys: {list(hidden)}")
        host.send({"type": "rule.addElement", "host": "news.com", "selector": "#a"})
        host.send({"type": "rule.addElement", "host": "news.com", "selector": "#b"})
        host.send({"type": "rule.removeElement", "host": "https://www.news.com/x", "selector": "#a"})
        wait_for(lambda: (host.latest_state() or {}).get("rules", {}).get("hiddenElements", {}).get("news.com") == ["#b"], 3)
        pushed = (host.latest_state() or {}).get("rules", {}).get("hiddenElements", {}).get("news.com")
        check("rule.removeElement pushes state without being asked", pushed == ["#b"], f"pushed news.com={pushed}")

        # D4: the override marks the event for the site that was allowed, not the newest one.
        host.send({"type": "blockEvent", "target": "https://www.youtube.com/watch?v=1", "layer": "rule", "overridden": False})
        host.send({"type": "blockEvent", "target": "https://reddit.com/r/all", "layer": "rule", "overridden": False})
        host.send({"type": "override.request", "id": "o1", "target": "youtube.com", "reason": "need the talk"})
        r = host.reply("o1")
        check("override.request is allowed in focus", bool(r and r.get("allowed")), json.dumps(r))
        events = {e["target"]: e["overridden"] for e in saved().get("blockEvents", [])}
        check("the youtube.com event is the one marked overridden",
              events.get("youtube.com") is True and events.get("reddit.com") is False, json.dumps(events))

        # D7: listening with no current task is kept as an unassigned total.
        host.send({"type": "read.log", "seconds": 30, "url": "https://example.com/a", "title": "A"})
        d = saved()
        listened = [t["listenedSec"] for t in d["tasks"]]
        check("read.log with no current task is kept, not dropped",
              d.get("unassignedListenedSec") == 30 and listened == [0],
              f"unassigned={d.get('unassignedListenedSec')} task listenedSec={listened}")

        # Privacy: the app refuses cloud voice for a page on the never-analyse list (the default
        # list has mail.google.com). The scratch profile has no cloud voice, so without the check
        # this would still be an error, but one that does not name the site.
        host.send({"type": "tts.speak", "id": "p1", "text": "You have 3 new emails.", "rate": 1,
                   "url": "https://mail.google.com/mail/u/0/#inbox"})
        r = host.reply("p1")
        check("tts.speak for a never-analyse page is refused with an error naming the site",
              r is not None and r.get("type") == "error" and "mail.google.com" in r.get("message", ""), json.dumps(r))

        # D1: a second launch on the same profile hands off and exits; the first keeps the socket.
        second = subprocess.Popen([os.path.join(APP, "Contents/MacOS/Kairos")], env=ENV,
                                  stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            code = second.wait(timeout=6)
        except subprocess.TimeoutExpired:
            code = None
            second.terminate()
        fresh = Host()
        fresh.send({"type": "getState", "id": "new-connection"})
        r = fresh.reply("new-connection")
        check("a second launch exits instead of starting", code == 0, f"exit code {code}")
        check("a new connection still reaches the first instance", r is not None and app.poll() is None)
        fresh.close()
        host.close()
    finally:
        app.terminate()
        app.wait(timeout=5)
    print(f"{sum(results)}/{len(results)} checks passed. Scratch profile: {HOME}")
    sys.exit(0 if all(results) else 1)


if __name__ == "__main__":
    main()
