import { writeDeclaration } from "./declaration-fixtures.ts";
// Persisted catalog inputs and large declarations. Changes happen before any app is
// launched; the source audit is restored after it ends, before domain fixture teardown.
import { prepareFixture } from "./build-fixtures.ts";
import { artifactize, cancelRuns, createChain, type Fixture } from "./fixture.ts";
import { git } from "./environment.ts";
import { makeDirectory, nativePath, updateDatabase } from "./platform.ts";

/** A readable legacy audit with no saved definitions or requests to draw. */
export function createEmptyAudit(): { fixture: Fixture; restore: () => void } {
  const fixture = createChain(1);
  cancelRuns(fixture);
  const database = nativePath(fixture.state, "state.sqlite");
  updateDatabase(database, (db) => {
    db.exec("CREATE TABLE fixture_requests AS SELECT * FROM requests; CREATE TABLE fixture_definitions AS SELECT * FROM run_definitions; DELETE FROM requests; DELETE FROM run_definitions;");
  });
  return { fixture, restore: () => updateDatabase(database, (db) => {
    db.exec("INSERT INTO requests SELECT * FROM fixture_requests; INSERT INTO run_definitions SELECT * FROM fixture_definitions; DROP TABLE fixture_requests; DROP TABLE fixture_definitions;");
  }) };
}

/** The comment of every Artifact of a commented layout fixture. */
export const CATALOG_NOTE = "Kept by the catalog's owner.";

/** Enough independent basis nodes to make a real worker layout observable after consent,
 * without launching hundreds of irrelevant fixture runtime processes; `commented`, each with a
 * comment. */
export function createLayout(wholes = 0, commented = false): Fixture {
  return prepareFixture((root) => {
    const fixture = { root, repo: makeDirectory(nativePath(root, "repo")), state: nativePath(root, "state") };
    for (let index = 0; index < 1200; index += 1) {
      const name = `n${String(index).padStart(4, "0")}`;
      const comment = commented ? `\n[annotations.comment]\ntext = ${JSON.stringify(CATALOG_NOTE)}\n` : "";
      writeDeclaration(fixture.repo, nativePath(makeDirectory(nativePath(fixture.repo, name)), "index.artf"), `name = "${name}"\nbasis = true\n${comment}`);
    }
    // A few wholes among them, each a box around two parts, when asked for.
    for (let index = 0; index < wholes; index += 1) {
      const whole = `w${String(index).padStart(2, "0")}`;
      writeDeclaration(fixture.repo, nativePath(makeDirectory(nativePath(fixture.repo, whole)), "index.artf"), `name = "${whole}"\nbasis = true\n`);
      for (const part of ["a", "b"]) {
        writeDeclaration(fixture.repo, nativePath(makeDirectory(nativePath(fixture.repo, whole, part)), "index.artf"), `name = "${whole}${part}"\nbasis = true\n`);
      }
    }
    git(fixture.repo, "init", "-q", "--initial-branch=master", fixture.repo);
    if (artifactize(fixture, "verify", "--all", "--wait", "--json") !== 0) throw new Error("the basis layout fixture did not verify");
    return fixture;
  });
}

/** The launch scope has no saved Run; ALL still has the fixture's reviewed repository. */
export function emptyRepository(fixture: Fixture): string {
  const repo = makeDirectory(nativePath(fixture.root, "empty-repo"));
  writeDeclaration(repo, nativePath(repo, "index.artf"), 'name = "empty"\nbasis = true\n');
  git(repo, "init", "-q", "--initial-branch=master", repo);
  return repo;
}
