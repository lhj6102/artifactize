import { createContext } from "svelte";

export class LeaveDialogState {
  working = $state(false);
  error = $state<string | null>(null);
}

export const [getLeaveDialogState, setLeaveDialogState] = createContext<LeaveDialogState>();
