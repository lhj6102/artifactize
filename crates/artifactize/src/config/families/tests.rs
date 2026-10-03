use serde_json::json;

use super::*;
use crate::config::read_workspace_config;

fn family() -> Value {
    json!({
        "name": "scenarios",
        "family": {"instances": {"alpha": {}, "beta": {}}},
        "evals": [{"id":"review", "title":"Review", "profile":{"kind":"runtime","command":"true","args":[]}, "payload":{"instruction":"Inspect."}}]
    })
}

fn discover(value: Value) -> Result<crate::config::RepoConfig, super::super::ConfigError> {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("scenarios")).unwrap();
    fs::write(
        root.path().join("scenarios").join(CONFIG_FILE),
        value.to_string(),
    )
    .unwrap();
    read_workspace_config(root.path())
}

#[test]
fn parameters_merge_shallowly_and_substitute_only_exact_json_values() {
    let mut value = family();
    value["family"] = json!({
        "params": {"default": true, "choice":"default", "object":{"old":true}, "array":["first",{"a/b":{"~key":42}}], "literal":{"$param":"/not-evaluated"}},
        "variants": {"custom":{"choice":"variant", "object":{"variant":true}}},
        "instances": {"alpha":{"variant":"custom", "params":{"choice":"instance", "object":{"instance":true}}}, "beta":{"variant":"custom"}, "gamma":{}}
    });
    value["evals"][0]["payload"] = json!({
        "instruction":"Inspect literal /choice and $param strings.",
        "root":{"$param":""}, "escaped":{"$param":"/array/1/a~1b/~0key"},
        "selected":[{"$param":"/choice"}], "literal":{"$param":"/literal"}
    });
    let mut config = discover(value).unwrap();
    let alpha = &config.evals[0].declaration.payload;
    assert_eq!(alpha["root"]["object"], json!({"instance":true}));
    assert_eq!(alpha["root"]["default"], true);
    assert_eq!(alpha["escaped"], 42);
    assert_eq!(alpha["selected"], json!(["instance"]));
    assert_eq!(alpha["literal"], json!({"$param":"/not-evaluated"}));
    let beta = &config.evals[1].declaration.payload;
    assert_eq!(beta["root"]["object"], json!({"variant":true}));
    assert_eq!(beta["selected"], json!(["variant"]));
    assert_eq!(
        config.evals[2].declaration.payload["root"]["object"],
        json!({"old":true})
    );
    config.evals[0].declaration.payload["root"]["array"][0] = json!("changed");
    assert_eq!(
        config.evals[1].declaration.payload["root"]["array"][0],
        "first"
    );
}

#[test]
fn pointers_reject_bad_escapes_noncanonical_array_indices_and_missing_values() {
    let params = json!({"": "empty key", "array":[null], "~1":true});
    assert_eq!(parameter(&params, "/").unwrap(), "empty key");
    assert_eq!(parameter(&params, "/array/0").unwrap(), &Value::Null);
    assert_eq!(parameter(&params, "/~01").unwrap(), true);
    for pointer in [
        "not/a/pointer",
        "/~",
        "/~2",
        "/array/01",
        "/array/+0",
        "/array/-",
        "/array/1",
        "/missing",
    ] {
        assert!(parameter(&params, pointer).is_err(), "{pointer}");
    }
    for mut reference in [json!({"$param": 0}), json!({"$param":"", "extra":true})] {
        assert!(
            substitute(&mut reference, &params)
                .unwrap_err()
                .contains("exactly")
        );
    }
    let mut value = family();
    value["evals"][0]["title"] = json!({"$param":"/missing"});
    assert!(
        discover(value.clone())
            .unwrap_err()
            .message
            .contains("Instance alpha: No parameter")
    );
    value["family"]["params"] = json!({"missing":false});
    assert!(
        discover(value).is_err(),
        "expanded declarations must be validated"
    );
    let ordinary = json!({"name":"plain", "evals":[{"id":"review","title":"Review","profile":{"kind":"human"},"payload":{"instruction":"Read", "data":{"$param":""}}}]});
    assert!(
        super::super::parse_declaration(&ordinary.to_string())
            .unwrap_err()
            .contains("only in an Artifact family")
    );
}

#[test]
fn family_and_instance_fields_are_strict_and_variants_must_exist() {
    let mut value = family();
    value["family"]["generator"] = json!("never-run.sh");
    assert!(discover(value).is_err());
    let mut value = family();
    value["family"]["params"] = Value::Null;
    assert!(discover(value).is_err());
    let mut value = family();
    value["family"]["variants"] = json!({"bad/name":{}});
    assert!(discover(value).is_err());
    let mut value = family();
    value["family"]["variants"] = json!({"custom":[]});
    assert!(discover(value).is_err());
    let mut value = family();
    value["family"]["instances"] = json!({"alpha":{"command":"never-run.sh"}});
    assert!(discover(value).is_err());
    let mut value = family();
    value["family"]["instances"] = json!({"alpha":{"variant":"missing"}});
    assert!(
        discover(value)
            .unwrap_err()
            .message
            .contains("Unknown family variant")
    );
    let mut value = family();
    value["family"]["instances"] = json!({"alpha":{"params":[]}});
    assert!(discover(value).is_err());
    let mut value = family();
    value["family"]["instances"] = json!({"alpha":{"variant":null}});
    assert!(discover(value).is_err());
}

#[test]
fn shared_fields_remain_literal_and_basis_applies_to_every_instance() {
    let mut value = family();
    value["family"]["params"] = json!({"command":"changed.sh"});
    value["stale"] =
        json!({"kind":"identity", "script":{"command":"identity.sh", "args":["$param"]}});
    value["basis"] = json!(true);
    value.as_object_mut().unwrap().remove("evals");
    let config = discover(value.clone()).unwrap();
    assert_eq!(
        config.review_requirement("alpha"),
        Some(crate::config::ReviewRequirement::Basis)
    );
    assert_eq!(
        config.review_requirement("beta"),
        Some(crate::config::ReviewRequirement::Basis)
    );
    assert!(config.evals.is_empty());
    assert_eq!(
        config.artifacts["alpha"].family.as_ref().unwrap().instances,
        None
    );
    let Some(crate::config::Stale::Identity { script, .. }) = &config.artifacts["alpha"].stale
    else {
        panic!()
    };
    assert_eq!(script.command, "identity.sh");
    assert_eq!(script.args, ["$param"]);
    value["stale"]["script"]["command"] = json!({"$param":"/command"});
    assert!(discover(value).is_err());
    let mut value = family();
    value["unknown"] = json!(true);
    assert!(
        discover(value)
            .unwrap_err()
            .message
            .contains("unknown field")
    );
}

#[test]
fn families_have_bounded_static_membership_and_material_lists() {
    let mut value = family();
    value["family"]["instances"] = json!({});
    assert!(
        discover(value.clone())
            .unwrap_err()
            .message
            .contains("1–10000")
    );
    let entries: Map<_, _> = (0..MAX_FAMILY_INSTANCES)
        .map(|i| (format!("instance{i}"), json!({})))
        .collect();
    value["family"]["instances"] = Value::Object(entries);
    assert_eq!(
        discover(value.clone()).unwrap().artifacts.len(),
        MAX_FAMILY_INSTANCES
    );
    value["family"]["instances"]["excess"] = json!({});
    assert!(discover(value).unwrap_err().message.contains("1–10000"));

    let root = tempfile::tempdir().unwrap();
    let owner = root.path().join("scenarios");
    fs::create_dir(&owner).unwrap();
    let material: Vec<_> = (0..64).map(|i| format!("file{i}")).collect();
    for file in &material {
        fs::write(owner.join(file), "material").unwrap();
    }
    let mut value = family();
    value["family"]["instances"] = json!({"alpha":{"material":material}});
    fs::write(owner.join(CONFIG_FILE), value.to_string()).unwrap();
    assert_eq!(
        read_workspace_config(root.path()).unwrap().artifacts["alpha"]
            .family
            .as_ref()
            .unwrap()
            .material
            .len(),
        64
    );
    value["family"]["instances"]["alpha"]["material"]
        .as_array_mut()
        .unwrap()
        .push(json!("file64"));
    fs::write(owner.join(CONFIG_FILE), value.to_string()).unwrap();
    assert!(
        read_workspace_config(root.path())
            .unwrap_err()
            .message
            .contains("at most 64")
    );
    value["family"]["instances"]["alpha"]["material"] = json!(["file0", "file0"]);
    fs::write(owner.join(CONFIG_FILE), value.to_string()).unwrap();
    assert!(
        read_workspace_config(root.path())
            .unwrap_err()
            .message
            .contains("unique")
    );
}

#[test]
fn membership_keeps_sorted_material_and_is_independent_of_siblings() {
    let root = tempfile::tempdir().unwrap();
    let owner = root.path().join("scenarios");
    fs::create_dir(&owner).unwrap();
    for file in ["a", "b", "c"] {
        fs::write(owner.join(file), file).unwrap();
    }
    let mut value = family();
    value["family"]["instances"] =
        json!({"alpha":{"material":["b","a"]}, "beta":{"material":["c"]}});
    fs::write(owner.join(CONFIG_FILE), value.to_string()).unwrap();
    let initial = read_workspace_config(root.path()).unwrap();
    let membership = initial.artifacts["alpha"].family.as_ref().unwrap();
    assert_eq!(membership.name, "scenarios");
    assert_eq!(membership.instances, None);
    assert_eq!(membership.material, ["a", "b"]);
    value["family"]["instances"]["beta"]["params"] = json!({"changed":true});
    value["family"]["instances"]["gamma"] = json!({});
    fs::write(owner.join("c"), "changed sibling material").unwrap();
    fs::write(owner.join(CONFIG_FILE), value.to_string()).unwrap();
    let updated = read_workspace_config(root.path()).unwrap();
    assert_eq!(
        membership,
        updated.artifacts["alpha"].family.as_ref().unwrap()
    );
}
