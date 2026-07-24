use db_app::cloudsync_table_registry;

fn is_rls_policy_denial(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    let mentions_rls = message.contains("row-level security")
        || message.contains("row level security")
        || message
            .split(|character: char| !character.is_ascii_alphanumeric())
            .any(|part| part == "rls");
    let mentions_denial = [
        "denied",
        "deny",
        "rejected",
        "forbidden",
        "violation",
        "failed",
        "failure",
        "not allowed",
        "not authorized",
    ]
    .iter()
    .any(|needle| message.contains(needle));

    mentions_rls && mentions_denial
}

#[test]
fn rls_policy_denial_matcher_rejects_generic_failures() {
    assert!(is_rls_policy_denial(
        "RLS policy denied INSERT on e2ee_records"
    ));
    assert!(is_rls_policy_denial(
        "row-level security policy check failed"
    ));
    assert!(!is_rls_policy_denial(
        "401 database_auth_failed: database credentials were rejected: Invalid APIKEY"
    ));
    assert!(!is_rls_policy_denial("connection timed out"));
    assert!(!is_rls_policy_denial("access token expired"));
    assert!(!is_rls_policy_denial("network retry policy failed"));
}

#[test]
fn cloudsync_enables_only_the_encrypted_replica() {
    let enabled_tables: Vec<&str> = cloudsync_table_registry()
        .iter()
        .filter(|table| table.enabled)
        .map(|table| table.table_name.as_str())
        .collect();

    assert_eq!(enabled_tables, ["e2ee_records"]);
}
