//! Forward migrations of older state schemas, applied before any read or write.

use std::{path::Path, time::Duration};

use rusqlite::{OpenFlags, TransactionBehavior};
use serde_json::{Map, Value};

use super::{STATE_SCHEMA_VERSION, receipts::Error};

/// Upgrade an existing older database in place; missing or current databases are untouched.
pub(super) fn upgrade(path: &Path) -> Result<(), Error> {
    if !path
        .try_exists()
        .map_err(|e| Error::Invalid(e.to_string()))?
    {
        return Ok(());
    }
    let version = {
        let db = rusqlite::Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        db.busy_timeout(Duration::from_secs(5))?;
        db.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))?
    };
    if version == 0 || version >= STATE_SCHEMA_VERSION {
        return Ok(());
    }
    let mut db = rusqlite::Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    db.busy_timeout(Duration::from_secs(5))?;
    let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // Another process may have upgraded between the two reads.
    let mut version: u32 =
        transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version == 1 {
        stale_keys(&transaction)?;
        version = 2;
    }
    if version == 2 {
        fingerprints(&transaction)?;
        version = 3;
    }
    if version == 3 {
        reuse_keys(&transaction)?;
        transaction.pragma_update(None, "user_version", 4)?;
    }
    transaction.commit()?;
    Ok(())
}

/// Version 4 keys reuse by `hash(eval strategy, (name, fingerprint) of each Artifact the eval
/// depends on)`. Earlier entries cannot be mapped to that key: their fingerprints mixed in
/// dependency entries and their Eval definition hash covered the profile. Their reuse mappings
/// are dropped, so the first verify after the upgrade reviews again; executions, Runs and
/// waiting Human requests stay readable, and a pending Human submission still settles its Run.
fn reuse_keys(db: &rusqlite::Transaction<'_>) -> Result<(), Error> {
    db.execute_batch(
        "DROP INDEX IF EXISTS active_fingerprint;
        DROP INDEX IF EXISTS cache_lru;
        DROP TABLE cache_entries;
        ALTER TABLE executions DROP COLUMN fingerprint;
        ALTER TABLE executions DROP COLUMN eval_def_hash;
        ALTER TABLE executions ADD COLUMN key TEXT;",
    )?;
    db.execute_batch(super::cache_entries::SCHEMA)?;
    // Saved content fingerprints lose `dependencies`, so a waiting Human request's recorded
    // scope still matches the current declarations.
    rewrite(db, "requests", |data| {
        if let Some(artifacts) = data.pointer_mut("/humanDefinition/artifacts") {
            content_dependencies(artifacts);
        }
    })?;
    rewrite(db, "runs", |data| {
        if let Some(artifacts) = data.pointer_mut("/definitions/artifacts") {
            content_dependencies(artifacts);
        }
    })
}

/// Saved Artifact declarations: a content `fingerprint` drops its `dependencies` scope.
fn content_dependencies(artifacts: &mut Value) {
    for artifact in artifacts
        .as_object_mut()
        .into_iter()
        .flat_map(Map::values_mut)
    {
        if let Some(fingerprint) = artifact
            .get_mut("fingerprint")
            .and_then(Value::as_object_mut)
            .filter(|fingerprint| !fingerprint.contains_key("script"))
        {
            fingerprint.remove("dependencies");
        }
    }
}

/// Version 2 renames the version 1 reuse key to staleKey in columns, the active index and saved JSON.
fn stale_keys(db: &rusqlite::Transaction<'_>) -> Result<(), Error> {
    db.execute_batch(
        "ALTER TABLE executions RENAME COLUMN identity TO stale_key;
        ALTER TABLE cache_entries RENAME COLUMN identity TO stale_key;
        DROP INDEX IF EXISTS active_identity;
        CREATE UNIQUE INDEX IF NOT EXISTS active_stale_key ON executions(stale_key,eval_def_hash) WHERE stale_key IS NOT NULL AND status IN ('RUNNING','WAITING_HUMAN');",
    )?;
    rewrite(db, "executions", |data| {
        rename(data, "identity", "staleKey")
    })?;
    rewrite(db, "requests", |data| {
        rename(data, "identity", "staleKey");
        if let Some(artifacts) = data.pointer_mut("/humanDefinition/artifacts") {
            declarations(artifacts);
        }
    })?;
    rewrite(db, "runs", |data| {
        if let Some(artifacts) = data.pointer_mut("/definitions/artifacts") {
            declarations(artifacts);
        }
        for artifact in data
            .pointer_mut("/validation/artifacts")
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
        {
            rename(artifact, "identity", "staleKeyKind");
            rename(artifact, "value", "staleKey");
        }
    })
}

/// Version 3 renames staleKey to fingerprint in columns, the active index and saved JSON;
/// saved declarations take the fingerprint shape.
fn fingerprints(db: &rusqlite::Transaction<'_>) -> Result<(), Error> {
    db.execute_batch(
        "ALTER TABLE executions RENAME COLUMN stale_key TO fingerprint;
        ALTER TABLE cache_entries RENAME COLUMN stale_key TO fingerprint;
        DROP INDEX IF EXISTS active_stale_key;
        CREATE UNIQUE INDEX IF NOT EXISTS active_fingerprint ON executions(fingerprint,eval_def_hash) WHERE fingerprint IS NOT NULL AND status IN ('RUNNING','WAITING_HUMAN');",
    )?;
    rewrite(db, "executions", |data| {
        rename(data, "staleKey", "fingerprint")
    })?;
    rewrite(db, "requests", |data| {
        rename(data, "staleKey", "fingerprint");
        if let Some(artifacts) = data.pointer_mut("/humanDefinition/artifacts") {
            fingerprint_declarations(artifacts);
        }
    })?;
    rewrite(db, "runs", |data| {
        if let Some(artifacts) = data.pointer_mut("/definitions/artifacts") {
            fingerprint_declarations(artifacts);
        }
        for artifact in data
            .pointer_mut("/validation/artifacts")
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
        {
            rename(artifact, "staleKeyKind", "fingerprintKind");
            rename(artifact, "staleKey", "fingerprint");
        }
    })
}

fn rewrite(
    db: &rusqlite::Transaction<'_>,
    table: &str,
    change: impl Fn(&mut Value),
) -> Result<(), Error> {
    let rows = db
        .prepare(&format!("SELECT id,data FROM {table}"))?
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut update = db.prepare(&format!("UPDATE {table} SET data=? WHERE id=?"))?;
    for (id, data) in rows {
        let mut value: Value = serde_json::from_str(&data)?;
        change(&mut value);
        update.execute(rusqlite::params![serde_json::to_string(&value)?, id])?;
    }
    Ok(())
}

fn rename(object: &mut Value, from: &str, to: &str) {
    if let Some(object) = object.as_object_mut()
        && let Some(value) = object.remove(from)
    {
        object.insert(to.to_owned(), value);
    }
}

/// Saved Artifact declarations: `stale: {kind, ...}` becomes `staleKey: {script|content: {...}}`.
fn declarations(artifacts: &mut Value) {
    for artifact in artifacts
        .as_object_mut()
        .into_iter()
        .flat_map(Map::values_mut)
    {
        let Some(artifact) = artifact.as_object_mut() else {
            continue;
        };
        let Some(stale) = artifact.remove("stale") else {
            continue;
        };
        let key = match stale {
            Value::Object(mut stale) => match stale.remove("kind").as_ref().and_then(Value::as_str)
            {
                Some("identity") => {
                    let mut script = match stale.remove("script") {
                        Some(Value::Object(script)) => script,
                        _ => Map::new(),
                    };
                    script.extend(stale);
                    serde_json::json!({ "script": script })
                }
                Some(kind) => serde_json::json!({ kind: stale }),
                None => Value::Object(stale),
            },
            other => other,
        };
        artifact.insert("staleKey".into(), key);
    }
}

/// Saved Artifact declarations: `staleKey: {content: {inputs, ...}}` becomes the plain
/// `fingerprint: {files, ...}` and `staleKey: {script: {inputs, ...}}` becomes
/// `fingerprint: {script: {files, ...}}`.
fn fingerprint_declarations(artifacts: &mut Value) {
    for artifact in artifacts
        .as_object_mut()
        .into_iter()
        .flat_map(Map::values_mut)
    {
        let Some(artifact) = artifact.as_object_mut() else {
            continue;
        };
        let Some(key) = artifact.remove("staleKey") else {
            continue;
        };
        let fingerprint = match key {
            Value::Object(mut key) => match key.remove("content") {
                Some(mut content) => {
                    rename(&mut content, "inputs", "files");
                    content
                }
                None => {
                    if let Some(script) = key.get_mut("script") {
                        rename(script, "inputs", "files");
                    }
                    Value::Object(key)
                }
            },
            other => other,
        };
        artifact.insert("fingerprint".into(), fingerprint);
    }
}
