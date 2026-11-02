// A worktree's terminal on screen: xterm.js attached to the worktree's terminal for as long as
// the element exists. The first frame redraws the whole screen, live output follows, typing and
// resizing go back; removing the element only detaches, and the terminal keeps running.
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import type { Attachment } from "svelte/attachments";
import { call } from "../ipc/client";
import type { Workbench, WorktreeKey } from "./backend";
import type { Terminals } from "./terminals.svelte";

/** The theme's colours, read from the page's tokens as the screen opens. */
function theme(element: HTMLElement) {
  const token = (name: string) => getComputedStyle(element).getPropertyValue(name).trim() || undefined;
  return { background: token("--surface"), foreground: token("--text"), cursor: token("--accent"), selectionBackground: token("--selection") };
}

export function screen(worktree: WorktreeKey, terminals: Terminals, backend: Workbench): Attachment<HTMLElement> {
  return (element) => {
    const terminal = new Terminal({
      cursorBlink: true,
      fontFamily: getComputedStyle(element).getPropertyValue("--mono").trim() || "monospace",
      fontSize: 13,
      scrollback: 5000,
      theme: theme(element),
    });
    const fit = new FitAddon();
    terminal.loadAddon(fit);
    terminal.open(element);
    fit.fit();
    let attached = true;
    const failed = (failure: unknown) => attached && terminals.failed(worktree, failure);
    call(
      backend.terminalOpen(worktree, terminal.cols, terminal.rows, (frame) => {
        if (!attached) return;
        // A snapshot resets the screen and redraws it at the host's size; the screen then fits
        // its box again, which resizes the host if they differ.
        if (frame.kind === "snapshot") {
          terminal.resize(frame.cols, frame.rows);
          terminal.write(new Uint8Array(frame.data), () => attached && fit.fit());
        } else if (frame.kind === "output") terminal.write(new Uint8Array(frame.data));
        terminals.heard(worktree, frame);
      }),
    ).catch(failed);
    const send = (bytes: number[]) => void call(backend.terminalInput(worktree, bytes)).catch(failed);
    const encoder = new TextEncoder();
    const typed = terminal.onData((data) => send([...encoder.encode(data)]));
    // Some mouse reports are bytes, one per character.
    const binary = terminal.onBinary((data) => send([...data].map((byte) => byte.charCodeAt(0) & 0xff)));
    const resized = terminal.onResize(({ cols, rows }) => void call(backend.terminalResize(worktree, cols, rows)).catch(failed));
    // The screen follows its box: the panel's width, the window's height.
    const observer = new ResizeObserver(() => fit.fit());
    observer.observe(element);
    return () => {
      attached = false;
      observer.disconnect();
      typed.dispose();
      binary.dispose();
      resized.dispose();
      void backend.terminalClose(worktree);
      terminal.dispose();
    };
  };
}
