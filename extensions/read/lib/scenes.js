// Saved 3D scenes. Each article's scenes live under their own storage key, so the side panel loads
// only the current article's; a small index keeps the newest MAX_SCENES across all articles.
import { positionKey } from "./sentences.js";

export const MAX_SCENES = 20;
const INDEX = "sceneIndex"; // [{key, id, at}], newest first

const keyFor = (url) => `scenes:${positionKey(url)}`;

/** @param {chrome.storage.StorageArea} area */
export async function scenesFor(area, url) {
  const key = keyFor(url);
  const got = await area.get(key);
  return got[key] || [];
}

export async function saveScene(area, url, entry, { max = MAX_SCENES, now = Date.now() } = {}) {
  const key = keyFor(url);
  const got = await area.get([key, INDEX]);
  const scene = { id: entry.id, subject: entry.subject, html: entry.html, at: now };
  const lists = { [key]: [scene, ...(got[key] || []).filter((s) => s.id !== entry.id)] };
  const index = [{ key, id: entry.id, at: now }, ...(got[INDEX] || []).filter((e) => !(e.key === key && e.id === entry.id))];
  for (const old of index.slice(max)) {
    if (!(old.key in lists)) lists[old.key] = (await area.get(old.key))[old.key] || [];
    lists[old.key] = lists[old.key].filter((s) => s.id !== old.id);
  }
  await write(area, lists, index.slice(0, max));
}

export async function deleteScene(area, url, id) {
  const key = keyFor(url);
  const got = await area.get([key, INDEX]);
  const lists = { [key]: (got[key] || []).filter((s) => s.id !== id) };
  await write(area, lists, (got[INDEX] || []).filter((e) => !(e.key === key && e.id === id)));
}

async function write(area, lists, index) {
  const empty = Object.keys(lists).filter((k) => lists[k].length === 0);
  for (const k of empty) delete lists[k];
  await area.set({ ...lists, [INDEX]: index });
  if (empty.length) await area.remove(empty);
}
