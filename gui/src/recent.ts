// What the search box offers before anything is typed. This webview's own storage, since nothing else reads it.
const KEY = "recent-searches";
const MAX = 8;

export function recentSearches(): string[] {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(KEY) ?? "[]");
    return Array.isArray(saved) ? saved.filter((s): s is string => typeof s === "string").slice(0, MAX) : [];
  } catch {
    return [];
  }
}

function save(list: string[]) {
  try {
    localStorage.setItem(KEY, JSON.stringify(list));
  } catch {
    // A private or full store costs the list, never the search.
  }
}

/** Moves `query` to the front, so searching again for something doesn't list it twice. */
export function rememberSearch(query: string) {
  const q = query.trim();
  if (!q) return;
  save([q, ...recentSearches().filter((s) => s.toLowerCase() !== q.toLowerCase())].slice(0, MAX));
}

export function forgetSearch(query: string) {
  save(recentSearches().filter((s) => s !== query));
}

export function clearSearches() {
  save([]);
}
