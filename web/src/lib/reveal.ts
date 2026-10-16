/** Scrolls `box` so that `row` is centred, but only when the row is not completely inside the visible area (so stepping within the viewport never moves the listing). */
export function revealRow(box: HTMLElement | null | undefined, row: Element | null | undefined) {
  if (!box || !row || box.clientHeight === 0) return;
  const b = box.getBoundingClientRect();
  const r = row.getBoundingClientRect();
  if (r.top >= b.top && r.bottom <= b.top + box.clientHeight) return;
  box.scrollTop += r.top - b.top - (box.clientHeight - r.height) / 2;
}
