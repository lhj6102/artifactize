/** Setup protocol watchdogs bound failures; they never turn elapsed time into success. */
const SETUP_TIMEOUT = 30000;
const RECONCILE_INTERVAL = 25;

export async function observe(check: () => boolean | Promise<boolean>, failure: () => string): Promise<void> {
  const deadline = Date.now() + SETUP_TIMEOUT;
  for (;;) {
    if (await check()) return;
    if (Date.now() >= deadline) throw new Error(failure());
    await new Promise((done) => setTimeout(done, RECONCILE_INTERVAL));
  }
}
