use serde_json::json;
use soapbox::{config::Principal, store::Store};

#[test]
fn idle_count_includes_old_payments_and_unclaimed_operator_quotes_without_cross_owner_leaks() {
    let s = Store::open(std::path::Path::new(":memory:"), 1000, 1024).unwrap();
    let insert = |id: &str, status: &str, owner: &str, operator: &str| {
        s.db.execute(
            "INSERT INTO jobs VALUES(?1,?2,?1,?3,?4)",
            rusqlite::params![
                id,
                owner,
                status,
                json!({"worker":null,"spec":{"mode":{"operator":operator}}}).to_string()
            ],
        )
        .unwrap();
    };
    insert("old-payment", "refund_pending", "buyer", "gpu");
    for i in 0..101 {
        insert(&format!("done-{i}"), "succeeded", "buyer", "gpu");
    }
    insert("quote", "awaiting_payment", "buyer", "gpu");
    insert("other", "running", "other-buyer", "other-gpu");
    let principal = |id: &str, role: &str| Principal {
        id: id.into(),
        token_env: "TEST".into(),
        roles: vec![role.into()],
        pools: vec![],
        projects: vec![],
    };
    assert_eq!(s.active_jobs(&principal("buyer", "submitter")).unwrap(), 2);
    assert_eq!(s.active_jobs(&principal("gpu", "worker")).unwrap(), 2);
    assert_eq!(s.active_jobs(&principal("gpu", "submitter")).unwrap(), 0);
    assert_eq!(
        s.active_jobs(&principal("unrelated", "submitter")).unwrap(),
        0
    );
    assert_eq!(s.active_jobs(&principal("admin", "admin")).unwrap(), 3);
}
