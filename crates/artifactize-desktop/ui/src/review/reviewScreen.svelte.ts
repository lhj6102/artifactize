import { createContext } from "svelte";
import type { DetailView, HumanView, Results } from "../ipc/client";
import type { Review } from "./review.svelte";

/** What the Review screen holds: the search and its answer, the open request and what shows of it. */
export class ReviewScreenState {
  query = $state("");
  start = $state(0);
  previous = $state<number[]>([]);
  results = $state.raw<Results | null>(null);
  error = $state<string | null>(null);
  index = $state(0);
  review = $state<Review | null>(null);
  record = $state.raw<DetailView | null>(null);
  technical = $state(false);
  pending = $state(false);
  details = $state(false);
  focus = $state<"list" | "detail">("list");
  preview = $state.raw<{ human: HumanView } | { record: DetailView } | null>(null);
  asking = $state(false);
  wanted = $state("");
  list = $state<HTMLElement>();
  field = $state<HTMLInputElement>();
  detailPane = $state<HTMLElement>();
  notice = $state<string | null>(null);
  /** The query last searched; a changed query searches again from the newest page. */
  searched: string;
  /** Each read supersedes the ones before it, so a slow answer never shows an older query. */
  reads = 0;
  /** The newest read; leaving the search waits for the answer the list will show. */
  loading: Promise<void> = Promise.resolve();
  /** The pending search after a pause in typing. */
  typing: ReturnType<typeof setTimeout> | undefined;

  constructor(search: string) {
    this.query = search;
    this.searched = search;
  }
}

export const [getReviewScreenState, setReviewScreenState] = createContext<ReviewScreenState>();
