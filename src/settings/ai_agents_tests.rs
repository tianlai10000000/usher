use super::*;
use crate::settings::{AppSettings, SettingsManager};

#[test]
fn legacy_single_agent_migrates_once_without_copying_credentials() {
    for provider in ["custom", "deepseek", "openai"] {
        let settings: AppSettings = toml::from_str(&format!(
            r#"[ai.agent]
provider = "{provider}"
base_url = "https://legacy.example/v1"
api_key = "synthetic-legacy-{provider}"
model = "legacy-model"
reply_language = "zh-Hans"
"#
        ))
        .unwrap();
        let configuration = &settings.ai.agent;
        assert_eq!(configuration.profiles.len(), 1);
        assert_eq!(configuration.active_id, provider);
        let active = configuration.active_profile().unwrap();
        assert_eq!(active.id, provider);
        assert!(active.name.is_empty());
        assert_eq!(
            active.connection.api_key,
            format!("synthetic-legacy-{provider}")
        );
        assert_eq!(active.connection.reply_language, "zh-CN");
        let serialized = toml::to_string(&settings).unwrap();
        assert!(serialized.contains("[[ai.agent.profiles]]"));
        assert_eq!(serialized.matches("synthetic-legacy-").count(), 1);
        let table: toml::Value = toml::from_str(&serialized).unwrap();
        assert!(table["ai"]["agent"].get("api_key").is_none());
        assert_eq!(
            toml::from_str::<AppSettings>(&serialized).unwrap().ai.agent,
            *configuration
        );
    }
}

#[test]
fn fixed_provider_registry_migrates_stable_ids_names_and_independent_credentials() {
    let configuration: AiAgentConfiguration = toml::from_str(
        r#"active_provider = "deepseek"
[profiles.custom]
name = "Local agent"
api_key = "synthetic-custom-only"
[profiles.deepseek]
name = "Work reviewer"
provider = "openai"
base_url = "https://deepseek.example/v1"
api_key = "synthetic-deepseek-only"
model = "review-model"
reply_language = "zh-Hans"
[profiles.openai]
name = "Writing assistant"
api_key = "synthetic-openai-only"
"#,
    )
    .unwrap();
    assert_eq!(configuration.profiles.len(), 3);
    assert_eq!(configuration.active_id, "deepseek");
    assert_eq!(
        configuration.active_profile().unwrap().name,
        "Work reviewer"
    );
    assert_eq!(configuration.active().unwrap().provider, "deepseek");
    assert_eq!(configuration.active().unwrap().reply_language, "zh-CN");
    assert_eq!(
        configuration.profile("custom").unwrap().connection.api_key,
        "synthetic-custom-only"
    );
    assert_eq!(
        configuration.profile("openai").unwrap().connection.provider,
        "openai"
    );
    let serialized = toml::to_string(&configuration).unwrap();
    assert_eq!(
        toml::from_str::<AiAgentConfiguration>(&serialized).unwrap(),
        configuration
    );
}

#[test]
fn arbitrary_profiles_round_trip_same_provider_and_headers_without_changing_identity() {
    let configuration: AiAgentConfiguration = toml::from_str(
        r#"active_id = "work"
[[profiles]]
id = "work"
name = "Work"
provider = "openai"
api_key = "synthetic-work-key"
model = "work-model"
[[profiles.headers]]
enabled = true
name = "Authorization"
value = "synthetic-custom-auth"
[[profiles.headers]]
enabled = false
name = "unfinished draft row"
value = "line\nbreak"
[[profiles]]
id = "personal"
name = "Personal"
provider = "openai"
api_key = "synthetic-personal-key"
model = "personal-model"
"#,
    )
    .unwrap();
    assert_eq!(configuration.profiles.len(), 2);
    assert_eq!(configuration.active_profile().unwrap().id, "work");
    assert_eq!(
        configuration
            .profile("personal")
            .unwrap()
            .connection
            .provider,
        "openai"
    );
    let serialized = toml::to_string(&configuration).unwrap();
    assert_eq!(
        toml::from_str::<AiAgentConfiguration>(&serialized).unwrap(),
        configuration
    );
    assert_eq!(configuration.profiles[0].connection.headers.len(), 2);
}

#[test]
fn missing_registry_is_empty_and_bad_ids_or_malformed_profiles_do_not_fall_back_to_legacy() {
    assert_eq!(
        toml::from_str::<AiAgentConfiguration>("").unwrap(),
        AiAgentConfiguration::default()
    );
    let invalid_active: AiAgentConfiguration = toml::from_str("active_id = 'missing'").unwrap();
    assert!(invalid_active.active_profile().is_none());
    assert!(invalid_active.active().is_none());
    for malformed in [
        "profiles = 'malformed'",
        "[[profiles]]\nname='missing id'",
        "[[profiles]]\nid='same'\n[[profiles]]\nid='same'",
    ] {
        assert!(toml::from_str::<AiAgentConfiguration>(malformed).is_err());
    }
}

#[test]
fn header_validation_rejects_injection_transport_controls_and_values_without_echoing_secrets() {
    for (name, value) in [
        ("Host", "synthetic-host-secret"),
        ("CONTENT-length", "123"),
        ("Transfer-Encoding", "chunked"),
        ("Connection", "keep-alive"),
        ("Proxy-Authorization", "synthetic-proxy"),
        ("X-Invalid\r\n", "synthetic-name-value"),
        ("invalid name", "synthetic-marker"),
        ("X-Valid", "synthetic-CRLF\r\nInjected: true"),
        ("", "synthetic-empty-name"),
    ] {
        let header = AiAgentHeader {
            enabled: true,
            name: name.into(),
            value: value.into(),
        };
        let error = header.validate().unwrap_err();
        assert!(!error.contains("synthetic"));
        assert!(!format!("{header:?}").contains(value));
        let disabled = AiAgentHeader {
            enabled: false,
            ..header
        };
        disabled.validate().unwrap();
    }
    for name in [
        "Authorization",
        "X-Api-Key",
        "Cookie",
        "Content-Type",
        "OpenAI-Organization",
    ] {
        AiAgentHeader {
            enabled: true,
            name: name.into(),
            value: "synthetic-normal-control".into(),
        }
        .validate()
        .unwrap();
    }
    let enabled: AiAgentHeader = toml::from_str("name='X-Default'\nvalue='literal'").unwrap();
    assert!(enabled.enabled);
    let mut profile = AiAgentProfile::from(AiAgentSettings::default());
    profile.connection.headers = vec![
        AiAgentHeader {
            enabled: false,
            ..Default::default()
        };
        MAX_AI_AGENT_HEADERS
    ];
    profile.validate().unwrap();
    profile.connection.headers.push(AiAgentHeader::default());
    assert!(profile.validate().is_err());
    profile.connection.headers = vec![AiAgentHeader {
        name: "X-Injection\r\n".into(),
        value: "synthetic".into(),
        enabled: true,
    }];
    assert!(
        profile.normalized().validate().is_err(),
        "normalization must not erase CRLF evidence"
    );
}

#[test]
fn manager_persists_all_agents_headers_and_active_identity_atomically() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.toml");
    let manager = SettingsManager::with_config_path(path.clone());
    let mut a = AiAgentProfile::from(AiAgentSettings::default());
    a.id = "a".into();
    a.name = " Local control ".into();
    a.connection.api_key = "synthetic-local-control".into();
    let mut b = a.clone();
    b.id = "b".into();
    b.name = " Reviewer ".into();
    b.connection.api_key = "synthetic-review-control".into();
    b.connection.base_url = " https://api.example.com/ ".into();
    b.connection.headers.push(AiAgentHeader {
        enabled: true,
        name: " X-User ".into(),
        value: " literal value ".into(),
    });
    manager.set_ai_agent_settings(AiAgentConfiguration {
        active_id: "a".into(),
        profiles: vec![a, b],
    });
    manager.save().unwrap();
    let restored = SettingsManager::with_config_path(path).get().ai.agent;
    assert_eq!(restored.active_id, "a");
    assert_eq!(restored.active_profile().unwrap().name, "Local control");
    let reviewer = restored.profile("b").unwrap();
    assert_eq!(reviewer.name, "Reviewer");
    assert_eq!(reviewer.connection.base_url, "https://api.example.com");
    assert_eq!(reviewer.connection.api_key, "synthetic-review-control");
    assert_eq!(reviewer.connection.headers[0].name, "X-User");
    assert_eq!(reviewer.connection.headers[0].value, " literal value ");
}
