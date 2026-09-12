use ai_bridge_gatekeeper_core::admin_config::{
    AdminAuditEntry, AdminConfig, AllowlistEntry, CriteriaPatternSet, PolicySymbolEntry,
    PolicyValidationError, SymbolDefinition,
};

#[test]
fn config_round_trip_through_toml() {
    let mut config = AdminConfig::default();
    config.allowlist.push(AllowlistEntry {
        name: "cargo".to_string(),
        path: "/usr/bin/cargo".to_string(),
        allowed: true,
        args: vec!["--version".to_string()],
    });
    config.symbols.push(SymbolDefinition {
        name: "OPS.TERM.RUN.LOCAL".to_string(),
        classification: "delegable".to_string(),
        description: "Run a local command".to_string(),
    });
    config.policy.push(PolicySymbolEntry {
        symbol: "OPS.TERM.RUN.LOCAL".to_string(),
        classification: "delegable".to_string(),
        auto_approve_seconds: 120,
    });
    config.criteria.push(CriteriaPatternSet {
        category: "secrets".to_string(),
        patterns: vec!["token".to_string(), "secret".to_string()],
    });

    let raw = toml::to_string(&config).unwrap();
    let parsed: AdminConfig = toml::from_str(&raw).unwrap();
    assert_eq!(parsed.allowlist.len(), 1);
    assert_eq!(parsed.symbols[0].name, "OPS.TERM.RUN.LOCAL");
    assert_eq!(parsed.policy[0].auto_approve_seconds, 120);
}

#[test]
fn unsafe_values_are_rejected() {
    let config = AdminConfig {
        allowlist: vec![AllowlistEntry {
            name: "wildcard".to_string(),
            path: "*".to_string(),
            allowed: true,
            args: vec![],
        }],
        symbols: vec![SymbolDefinition {
            name: "CRIT.SECRET".to_string(),
            classification: "delegable".to_string(),
            description: "secret".to_string(),
        }],
        policy: vec![PolicySymbolEntry {
            symbol: "CRIT.SECRET".to_string(),
            classification: "delegable".to_string(),
            auto_approve_seconds: 0,
        }],
        criteria: vec![CriteriaPatternSet {
            category: "secrets".to_string(),
            patterns: vec!["secret".to_string()],
        }],
        sockets: vec![],
        branches: vec![],
        audit: vec![],
    };

    let err = config.validate().unwrap_err();
    assert!(matches!(err, PolicyValidationError::UnsafeAllowlistValue { .. }));
}

#[test]
fn audit_entries_are_created_on_changes() {
    let mut config = AdminConfig::default();
    let before = config.clone();
    config.allowlist.push(AllowlistEntry {
        name: "cargo".to_string(),
        path: "/usr/bin/cargo".to_string(),
        allowed: true,
        args: vec![],
    });
    config.audit.push(AdminAuditEntry {
        timestamp: "2026-08-31T00:00:00Z".to_string(),
        action: "allowlist.add".to_string(),
        summary: "Added cargo to allowlist".to_string(),
        old_value: toml::to_string(&before).unwrap(),
        new_value: toml::to_string(&config).unwrap(),
    });
    assert_eq!(config.audit.len(), 1);
    assert!(config.audit[0].summary.contains("cargo"));
}
