// Interrupting the declared Linux WebKitGTK test holder releases its private display,
// native app/driver resources and fixture. Other OSes have different native automation;
// on Linux missing capabilities fail helper setup, never become skipped coverage.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { holderUnsupported, holdSession } from "./holder.ts";
import { withCleanup } from "./owned.ts";

describe("an interrupted test process", { skip: holderUnsupported }, () => {
  for (const interruption of ["terminate", "kill"] as const) {
    it(`leaves nothing running after ${interruption}`, async () => {
      const holder = await holdSession();
      await withCleanup(async () => {
        assert.ok(holder.privateDisplay, "a private display");
        assert.ok(holder.nativeActive(), "the driver's and display's actual native owners run");
        assert.ok(holder.displayActive(), "the private display listens");
        holder.interrupt(interruption);
        await holder.waitReleased();
        assert.ok(holder.nativeExited(), "all actual native owners exited after the interruption");
        assert.ok(holder.displayClosed(), "the private display closed");
        assert.ok(holder.displayUnlocked(), "the private display unlocked");
      }, () => holder.stop());
    });
  }
});
