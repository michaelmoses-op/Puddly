//! Step 5 auth library tests — no HTTP server, no live Postgres.

use puddly_auth::{
    check_rule, check_rules, clear_session_cookie_header, hash_password, parse_session_cookie,
    resolve_user, session_cookie_header, verify_password, AccessDecision, AuthProvider, AuthUser,
    EmailPasswordProvider, MemorySessionStore, RegisterInput, RequestView, RowValues,
    SessionStore, AUTH_SCHEMA_SQL,
};
use puddly_db::{DbRow, RecordingExecutor, SqlValue};
use puddly_ir::{RuleIr, Span};

#[test]
fn auth_schema_sql_mentions_accounts_and_sessions() {
    assert!(AUTH_SCHEMA_SQL.contains("puddly_auth_accounts"));
    assert!(AUTH_SCHEMA_SQL.contains("puddly_sessions"));
}

#[tokio::test]
async fn memory_session_roundtrip_and_resolve() {
    let mut store = MemorySessionStore::new();
    let user = AuthUser::new("u1", "a@b.co").with_roles(vec!["admin".into()]);
    let token = store.create(&user, 3600).await.unwrap();
    assert_ne!(token.len(), 0);

    let got = store.get(&token).await.unwrap().expect("session");
    assert_eq!(got.user, user);

    let req = RequestView::with_token(&token);
    let resolved = resolve_user(&mut store, &req).await.expect("user");
    assert_eq!(resolved.id, "u1");

    store.delete(&token).await.unwrap();
    assert!(store.get(&token).await.unwrap().is_none());
    assert!(resolve_user(&mut store, &req).await.is_none());
}

#[test]
fn cookie_header_helpers() {
    let set = session_cookie_header("tok123", 60, true);
    assert!(set.contains("puddly_session=tok123"));
    assert!(set.contains("HttpOnly"));
    assert!(set.contains("Secure"));
    assert!(set.contains("SameSite=Lax"));

    let header = "other=1; puddly_session=tok123; x=y";
    assert_eq!(parse_session_cookie(header).as_deref(), Some("tok123"));

    let clear = clear_session_cookie_header(false);
    assert!(clear.contains("Max-Age=0"));
    assert!(!clear.contains("Secure"));
}

#[tokio::test]
async fn register_and_login_with_recording_executor() {
    let mut exec = RecordingExecutor::new();
    let mut sessions = MemorySessionStore::new();

    // register: existence query → empty
    exec.push_canned(vec![]);
    {
        let mut provider = EmailPasswordProvider::new(&mut exec, &mut sessions);
        let user = provider
            .register(RegisterInput {
                email: "  Alice@Example.COM ".into(),
                password: "password123".into(),
                user_id: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".into(),
                roles: vec!["editor".into()],
            })
            .await
            .unwrap();
        assert_eq!(user.email, "alice@example.com");
    }
    assert!(exec
        .log
        .iter()
        .any(|q| q.sql.contains("INSERT INTO \"puddly_auth_accounts\"")));

    // login: return the stored account row
    let hash = hash_password("password123").unwrap();
    let mut row = DbRow::new();
    row.insert(
        "user_id",
        SqlValue::Uuid("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".into()),
    );
    row.insert("email", SqlValue::Text("alice@example.com".into()));
    row.insert("password_hash", SqlValue::Text(hash.clone()));
    row.insert("roles", SqlValue::Text(r#"["editor"]"#.into()));
    exec.push_canned(vec![row]);

    let token = {
        let mut provider = EmailPasswordProvider::new(&mut exec, &mut sessions);
        let (logged_in, token) = provider
            .login("alice@example.com", "password123")
            .await
            .unwrap();
        assert_eq!(logged_in.id, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
        assert!(logged_in.has_role("editor"));
        assert!(!token.is_empty());
        token
    };

    // wrong password
    let mut row2 = DbRow::new();
    row2.insert(
        "user_id",
        SqlValue::Uuid("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".into()),
    );
    row2.insert("email", SqlValue::Text("alice@example.com".into()));
    row2.insert("password_hash", SqlValue::Text(hash));
    row2.insert("roles", SqlValue::Text(r#"["editor"]"#.into()));
    exec.push_canned(vec![row2]);
    {
        let mut provider = EmailPasswordProvider::new(&mut exec, &mut sessions);
        assert!(provider
            .login("alice@example.com", "wrong-password")
            .await
            .is_err());
    }

    // session from successful login still resolves
    assert!(resolve_user(&mut sessions, &RequestView::with_token(&token))
        .await
        .is_some());
}

#[tokio::test]
async fn login_unknown_email_is_invalid_credentials() {
    let mut exec = RecordingExecutor::new();
    exec.push_canned(vec![]); // no account
    let mut sessions = MemorySessionStore::new();
    let mut provider = EmailPasswordProvider::new(&mut exec, &mut sessions);
    let err = provider
        .login("nobody@example.com", "password123")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("invalid credentials"));
}

#[test]
fn access_rules_authenticated_owner_role_member() {
    let span = Span::dummy();
    let user = AuthUser::new("owner-1", "o@x.co").with_roles(vec!["admin".into()]);
    let row = RowValues::new().set("owner", "owner-1").set("org", "owner-1");

    assert_eq!(
        check_rule(&RuleIr::Authenticated { span }, Some(&user), None),
        AccessDecision::Allow
    );
    assert_eq!(
        check_rule(&RuleIr::Authenticated { span }, None, None),
        AccessDecision::Deny
    );

    assert_eq!(
        check_rule(
            &RuleIr::Owner {
                span,
                field: "owner".into()
            },
            Some(&user),
            Some(&row)
        ),
        AccessDecision::Allow
    );
    assert_eq!(
        check_rule(
            &RuleIr::Owner {
                span,
                field: "owner".into()
            },
            Some(&AuthUser::new("other", "z@z.co")),
            Some(&row)
        ),
        AccessDecision::Deny
    );

    assert_eq!(
        check_rule(
            &RuleIr::Role {
                span,
                role: "admin".into()
            },
            Some(&user),
            None
        ),
        AccessDecision::Allow
    );
    assert_eq!(
        check_rule(
            &RuleIr::Role {
                span,
                role: "viewer".into()
            },
            Some(&user),
            None
        ),
        AccessDecision::Deny
    );

    // Member: sync check fails closed; join-table EXISTS is RPC (ADR-047).
    assert_eq!(
        check_rule(
            &RuleIr::Member {
                span,
                via: "projectMember".into(),
                on: "project".into(),
                user_field: "user".into(),
            },
            Some(&user),
            Some(&row)
        ),
        AccessDecision::Deny
    );

    // Empty rule list fails closed
    assert_eq!(check_rules(&[], Some(&user), Some(&row)), AccessDecision::Deny);

    let and_rules = vec![
        RuleIr::Authenticated { span },
        RuleIr::Role {
            span,
            role: "admin".into(),
        },
    ];
    assert_eq!(
        check_rules(&and_rules, Some(&user), None),
        AccessDecision::Allow
    );
}

#[test]
fn password_verify_rejects_garbage_hash() {
    assert!(verify_password("x", "not-a-phc-hash").is_err());
}
