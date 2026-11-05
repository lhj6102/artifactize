<script lang="ts">
  // The star beside an Artifact's Detail title: ☆ stars the Artifact, a favorite of every branch
  // of its project, and ★ takes the star away. Starred Artifacts are what the graph's find field
  // lists with nothing typed.
  import type { Attachment } from "svelte/attachments";
  import { call, message, useIpc, type BranchKey } from "../ipc/client";
  import { ArtifactStarState, setArtifactStarState } from "./artifact-star-state.svelte";
  import { keyOf } from "./monitor-state.svelte";

  let { branch, artifact }: { branch: BranchKey; artifact: string } = $props();
  const commands = useIpc();
  const s = new ArtifactStarState();
  setArtifactStarState(s);

  /** Starred as the library keeps it; a click shows its change at once until the answer. */
  let on = $derived(s.starred?.branch === keyOf(branch) && s.starred.names.includes(artifact));

  /** Read the project's stars for the branch shown. */
  const read: Attachment<HTMLElement> = () => {
    const at = branch;
    let current = true;
    const asked = ++s.asked;
    call(commands.starredArtifacts(at)).then(
      (names) => {
        if (!current || asked !== s.asked) return;
        s.starred = { branch: keyOf(at), names };
        s.error = null;
      },
      (failure) => {
        if (current && asked === s.asked) s.error = message(failure);
      },
    );
    return () => {
      current = false;
    };
  };

  async function toggle() {
    const [at, wanted, asked] = [branch, !on, ++s.asked];
    on = wanted;
    try {
      const names = await call(commands.starArtifact(at, artifact, wanted));
      if (asked !== s.asked) return;
      s.starred = { branch: keyOf(at), names };
      s.error = null;
    } catch (failure) {      if (asked !== s.asked) return;
      // Nothing changed in the library: show its stars as last read, not the click's opposite,
      // which overlapping failed clicks would get wrong.
      if (s.starred) s.starred = { ...s.starred };
      else on = false;
      s.error = message(failure);
    }
  }
</script>

<button class="star" class:on aria-label="Star {artifact}" aria-pressed={on} {@attach read} onclick={() => void toggle()}>{on ? "★" : "☆"}</button>
{#if s.error}<span class="error" role="status">{s.error}</span>{/if}

<style>
  /* Quiet until it is on; on, it carries the amber of attention, as a branch's star does. */
  .star {
    flex: none;
    margin: 0;
    padding: 0 2px;
    border: 0;
    background: none;
    color: var(--muted);
    font-size: 14px;
    line-height: 1.4;
    cursor: pointer;
    opacity: 0.55;
  }
  .star:hover,
  .star.on {
    opacity: 1;
  }
  .star.on {
    color: var(--light-active);
  }
  .error {
    min-width: 0;
    color: var(--red);
    font-size: 12px;
    overflow-wrap: anywhere;
  }
</style>
