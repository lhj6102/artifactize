// artifactize passes the Live URI as argv; this observer, not artifactize, opens it.
const address = process.argv[2];
try {
  const response = await fetch(address, { signal: AbortSignal.timeout(25000) });
  await response.body?.cancel();
  if (response.status !== 200) throw new Error(`expected HTTP 200; got ${response.status}`);
  console.log(`${address}: HTTP 200`);
} catch (error) {
  console.error(`${address}: ${error.message}`);
  process.exitCode = 1;
}
