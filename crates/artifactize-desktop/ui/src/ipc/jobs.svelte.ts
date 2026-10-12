// The numbers of the cancellable Human jobs of this window: one owner, so no two jobs share one.
export class Jobs {
  last = $state(0);

  /** A fresh number for a cancellable Human job. */
  next(): number {
    this.last += 1;
    return this.last;
  }
}
