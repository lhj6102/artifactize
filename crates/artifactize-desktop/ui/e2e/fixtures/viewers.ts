// Projects whose Artifacts open in Artifact Viewers: each Artifact of a viewer type (`files`,
// `markdown`, `artifactize`, the `html` example plugin) with a Human eval whose review offers the
// type's viewer tool, made only through the real CLI and Git.
import { makeDirectory, nativePath, suitePath, writeText } from "../portable.ts";
import { check, declareArtifact, humanEval, indent } from "./declarations.ts";
import type { Owned } from "./owned.ts";
import { artifactize, initRepo, newFixture, verifyWaiting, type Fixture } from "./repo.ts";

/** A Human eval `signoff` of `what`. */
const signoff = (what: string) => humanEval("signoff", `A person reads ${what}`, `Read ${what}; submit RED with an issue for each problem.`);

export const NOTES = "notes-" + "W".repeat(58);

/** The notes Artifact, of type `files`: a file and a folder holding another. */
export async function createNotes(own: Owned): Promise<Fixture> {
  const fixture = newFixture(own);
  declareArtifact(fixture.repo, ["notes"], NOTES, { tools: "files", body: signoff("the notes") });
  writeText(nativePath(fixture.repo, "notes", "release.md"), "# Release\nTotals are rounded.\nShip it.\n");
  writeText(nativePath(makeDirectory(nativePath(fixture.repo, "notes", "drafts")), "old.txt"), "An old draft.\n");
  initRepo(fixture);
  await verifyWaiting(fixture);
  return fixture;
}

/** A guide of two Markdown files, a system of two declared Artifacts (back reads front) under an
 * `artifactize` Artifact, and a page shown by the `html` example plugin, installed into the
 * repository as a project's own plugin. */
export async function createViewed(own: Owned): Promise<Fixture> {
  const fixture = newFixture(own);
  declareArtifact(fixture.repo, ["guide"], "guide", { tools: "markdown", body: signoff("the guide") });
  writeText(nativePath(fixture.repo, "guide", "install.md"), "# Install\n\nRun the `installer`.\n\n- one\n- two\n");
  writeText(nativePath(fixture.repo, "guide", "usage.md"), "# Usage\n\nOpen the app.\n");
  // The system: its two Artifacts' definitions in a folder of .artifactize/, which the system covers.
  const definitions = makeDirectory(nativePath(fixture.repo, ".artifactize", "system"));
  writeText(nativePath(definitions, "front.artf"), `artf front = {\n  path = "front/"\n${indent(check("lint", "front"))}\n}\n`);
  writeText(nativePath(definitions, "back.artf"), `artf back = {\n  path = "back/"\n${indent(check("serves", "back", "pass", "Check that {front} is served."))}\n}\n`);
  for (const folder of ["front", "back"]) writeText(nativePath(makeDirectory(nativePath(fixture.repo, folder)), ".gitkeep"), "");
  writeText(nativePath(fixture.repo, ".artifactize", "system.artf"), `artf system : artifactize = {\n  path = ".artifactize/system/"\n${indent(signoff("the system"))}\n}\n`);
  declareArtifact(fixture.repo, ["checkout"], "page", { tools: "html", body: signoff("the page") });
  writeText(nativePath(fixture.repo, "checkout", "index.html"), '<!doctype html><title>Checkout</title><div id="checkout"><span class="total">42</span></div>\n');
  initRepo(fixture);
  const added = artifactize(fixture, ["plugin", "add", suitePath("..", "..", "..", "..", "examples", "plugins", "html")]);
  if (added.status !== 0) throw new Error(`the html plugin was not added: ${added.stderr}${added.stdout}`);
  await verifyWaiting(fixture);
  return fixture;
}
