//! Earlier state schemas, made from a current state, for upgrade tests.

/// Turn a schema-9 state into schema 8: a Run's definitions go back into its record, and the
/// revisions, their triggers and their indexes go away. The caller sets `user_version`.
pub const EIGHT: &str = "DROP TRIGGER run_revision; DROP TRIGGER request_revision; DROP TRIGGER execution_revision;
    DROP INDEX request_status; DROP INDEX request_marks; DROP INDEX request_execution; DROP INDEX request_claim; DROP INDEX execution_marks;
    UPDATE runs SET data=json_set(data,'$.definitions',json((SELECT d.data FROM run_definitions d WHERE d.run_id=runs.id))) WHERE id IN (SELECT run_id FROM run_definitions);
    DROP TABLE run_definitions;
    ALTER TABLE runs DROP COLUMN revision; ALTER TABLE requests DROP COLUMN revision; ALTER TABLE executions DROP COLUMN revision;";
