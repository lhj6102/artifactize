// Persisted catalog inputs and large declarations. Changes happen before any app is
// launched; the source audit is restored after it ends, before domain fixture teardown.
import { prepareFixture } from "./build-fixtures.ts";
import { artifactize, cancelRuns, createChain, type Fixture } from "./fixture.ts";
import { git } from "./environment.ts";
import { makeDirectory, nativePath, updateDatabase, writeText } from "./platform.ts";

/** A readable legacy audit with no saved definitions or requests to draw. */
export function createEmptyAudit(): { fixture: Fixture; restore: () => void } {
  const fixture = createChain(1);
  cancelRuns(fixture);
  const database = nativePath(fixture.state, "state.sqlite");
  updateDatabase(database, (db) => {
    db.exec("CREATE TABLE fixture_requests AS SELECT * FROM requests; CREATE TABLE fixture_runs AS SELECT * FROM runs; DELETE FROM requests; UPDATE runs SET data=json_set(data,'$.definitions',json('{}'));");
  });
  return { fixture, restore: () => updateDatabase(database, (db) => {
    db.exec("INSERT INTO requests SELECT * FROM fixture_requests; UPDATE runs SET data=(SELECT data FROM fixture_runs WHERE fixture_runs.id=runs.id); DROP TABLE fixture_requests; DROP TABLE fixture_runs;");
  }) };
}

/** Enough independent basis nodes to make a real worker layout observable after consent,
 * without launching hundreds of irrelevant fixture runtime processes. */
export function createLayout(): Fixture {
  return prepareFixture((root) => {
    const fixture = { root, repo: makeDirectory(nativePath(root, "repo")), state: nativePath(root, "state") };
    for (let index = 0; index < 1200; index += 1) {
      const name = `n${String(index).padStart(4, "0")}`;
      writeText(nativePath(makeDirectory(nativePath(fixture.repo, name)), "index.artf"), `name = "${name}"\nbasis = true\n`);
    }
    git(fixture.repo, "init", "-q", "--initial-branch=master", fixture.repo);
    if (artifactize(fixture, "verify", "--all", "--wait", "--json") !== 0) throw new Error("the basis layout fixture did not verify");
    return fixture;
  });
}

/** The launch scope has no saved Run; ALL still has the fixture's reviewed repository. */
export function emptyRepository(fixture: Fixture): string {
  const repo = makeDirectory(nativePath(fixture.root, "empty-repo"));
  writeText(nativePath(repo, ".artf"), 'name = "empty"\nbasis = true\n');
  git(repo, "init", "-q", "--initial-branch=master", repo);
  return repo;
}
