// The views of the library tab in the order they were seen, the one in front
// first: the list (null) and the PDFs open (the id of their paper). Closing the
// PDF in front shows the view seen before it, as a browser's history does; the
// list is always there, and is what is left at the end.

/** a view of the library tab: the PDF of a paper (its id), or the list (null) */
export type Seen = string | null;

/** The list at the end, unless it is in the order already. */
const withList = (order: Seen[]): Seen[] => (order.includes(null) ? order : [...order, null]);

/** The order with `view` in front: brought to the front, or added there. */
export function seen(order: Seen[], view: Seen): Seen[] {
  return withList([view, ...order.filter((v) => v !== view)]);
}

/** The order without the PDFs of `ids`, closed; its first view is the one to show if the one in front was closed. */
export function closed(order: Seen[], ids: Iterable<string>): Seen[] {
  const gone = new Set(ids);
  return withList(order.filter((v) => v === null || !gone.has(v)));
}

/** The order as the window opens, which no history is kept of: the view in front, then the other papers open from
 *  the last tab back, then the list. */
export function initial(open: string[], active: Seen): Seen[] {
  return seen(withList([...open].reverse()), active);
}
