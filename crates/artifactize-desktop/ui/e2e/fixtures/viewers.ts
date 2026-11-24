// Projects whose Artifacts open in Artifact Viewers: each Artifact of a viewer type (`files`,
// `markdown`, `artifactize`, the `html` example plugin) with a Human eval whose review offers the
// type's viewer tool, made only through the real CLI and Git.
import { makeDirectory, nativePath, suitePath, writeText } from "../portable.ts";
import { check, declareArtifact } from "./declarations.ts";
import type { Owned } from "./owned.ts";
import { artifactize, initRepo, newFixture, verifyWaiting, type Fixture } from "./repo.ts";

/** A Human eval `signoff` asking for a comment. */
const signoff = (what: string) => `
[evals.signoff]
title = ${JSON.stringify(`A person reads ${what}`)}
profile = { kind = "human" }
payload.instruction = ${JSON.stringify(`Read ${what} and say what you found.`)}

[evals.signoff.pass_schema]
type = "object"
properties.comment = { type = "string", minLength = 1, description = "What you checked." }
required = ["comment"]
additionalProperties = false

[evals.signoff.fail_schema]
type = "object"
properties.reason = { type = "string", minLength = 3 }
required = ["reason"]
additionalProperties = false
`;

export const NOTES = "notes-" + "W".repeat(58);

/** The notes Artifact, of type `files`: a file and a folder holding another. */
export async function createNotes(own: Owned): Promise<Fixture> {
  const fixture = newFixture(own);
  declareArtifact(fixture.repo, ["notes"], NOTES, `type = "files"\n${signoff("the notes")}`);
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
  declareArtifact(fixture.repo, ["guide"], "guide", `type = "markdown"\n${signoff("the guide")}`);
  writeText(nativePath(fixture.repo, "guide", "install.md"), "# Install\n\nRun the `installer`.\n\n- one\n- two\n");
  writeText(nativePath(fixture.repo, "guide", "usage.md"), "# Usage\n\nOpen the app.\n");
  // The system: its two Artifacts' definitions in a folder of .artifactize/, which the system covers.
  const definitions = makeDirectory(nativePath(fixture.repo, ".artifactize", "system"));
  writeText(nativePath(definitions, "front.artf"), `path = "front/"\n${check("lint", "front")}`);
  writeText(nativePath(definitions, "back.artf"), `path = "back/"\n${check("serves", "back", "pass", "Check that {front} is served.")}`);
  for (const folder of ["front", "back"]) writeText(nativePath(makeDirectory(nativePath(fixture.repo, folder)), ".gitkeep"), "");
  writeText(nativePath(fixture.repo, ".artifactize", "system.artf"), `path = ".artifactize/system/"\ntype = "artifactize"\n${signoff("the system")}`);
  declareArtifact(fixture.repo, ["checkout"], "page", `type = "html"\n${signoff("the page")}`);
  writeText(nativePath(fixture.repo, "checkout", "index.html"), '<!doctype html><title>Checkout</title><div id="checkout"><span class="total">42</span></div>\n');
  initRepo(fixture);
  const added = artifactize(fixture, ["plugin", "add", suitePath("..", "..", "..", "..", "examples", "plugins", "html")]);
  if (added.status !== 0) throw new Error(`the html plugin was not added: ${added.stderr}${added.stdout}`);
  await verifyWaiting(fixture);
  return fixture;
}
