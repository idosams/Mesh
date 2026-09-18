export const BOUNDED_CHOICE_LIMIT = 500;

export type BoundedChoiceProjection<T> = Readonly<{
  items: readonly T[];
  matched: number;
  offset: number;
  retainedSelected: boolean;
  truncated: boolean;
}>;

export function boundedChoiceProjection<T>(
  items: readonly T[],
  filterText: string,
  selectedIdentity: string,
  identity: (item: T) => string,
  searchableText: (item: T) => readonly string[],
  maximum = BOUNDED_CHOICE_LIMIT,
): BoundedChoiceProjection<T> {
  if (!Number.isSafeInteger(maximum) || maximum < 1) throw new Error("The visible choice-row bound was invalid.");
  const query = filterText.trim().toLocaleLowerCase();
  const matches = items.filter((item) => !query || searchableText(item)
    .some((value) => value.toLocaleLowerCase().includes(query)));
  const selected = items.find((item) => identity(item) === selectedIdentity) ?? null;
  const selectedIndex = selected ? matches.findIndex((item) => identity(item) === selectedIdentity) : -1;
  const retainedSelected = Boolean(selected && selectedIndex === -1);
  const capacity = retainedSelected ? Math.max(0, maximum - 1) : maximum;
  const pageOffset = selectedIndex >= capacity && capacity > 0
    ? Math.floor(selectedIndex / capacity) * capacity
    : 0;
  const offset = Math.min(pageOffset, Math.max(0, matches.length - capacity));
  const visibleMatches = matches.slice(offset, offset + capacity);
  const visible = retainedSelected && selected ? [selected, ...visibleMatches] : visibleMatches;
  return Object.freeze({
    items: Object.freeze(visible),
    matched: matches.length,
    offset,
    retainedSelected,
    truncated: matches.length > visibleMatches.length,
  });
}
