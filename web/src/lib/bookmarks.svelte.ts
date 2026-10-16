// Bookmarks store the locator they were made at; their step is looked up again after the recording is replaced.

import { api } from "./api";
import { lab, type Bookmark } from "./state.svelte";

/** Step of a bookmark in the current recording by bookmark id; `null` when its locator no longer resolves. */
const resolved = $state<Record<string, number | null>>({});
/** What each entry of `resolved` was resolved for: the generation and the locator. */
const stamps: Record<string, string> = {};

/** The step of `b` in the current recording, `null` if its locator is gone. Until it is looked up, the step saved with the bookmark. */
export function stepOf(b: Bookmark): number | null {
  return b.id in resolved ? resolved[b.id] : b.step;
}

/** Looks up every bookmark whose locator has not been resolved in the current generation. Reads the store, so call it inside an effect. */
export async function resolveBookmarks(): Promise<void> {
  const generation = lab.status.generation;
  const due = (lab.notes.bookmarks ?? []).map((b) => ({ id: b.id, locator: b.locator })).filter((b) => stamps[b.id] !== `${generation}|${b.locator}`);
  await Promise.all(
    due.map(async ({ id, locator }) => {
      let step: number | null;
      try {
        step = (await api.locate(locator)).step;
      } catch {
        step = null;
      }
      if (lab.status.generation !== generation) return; // the recording changed again; the effect will run again
      stamps[id] = `${generation}|${locator}`;
      resolved[id] = step;
    }),
  );
}
