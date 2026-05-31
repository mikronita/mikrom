export function normalizeSearch(value: string) {
  return value.trim().toLowerCase();
}

export function matchesSearch(values: Array<string | null | undefined>, query: string) {
  const normalizedQuery = normalizeSearch(query);
  if (!normalizedQuery) return true;

  return values.some((value) => normalizeSearch(value ?? "").includes(normalizedQuery));
}
