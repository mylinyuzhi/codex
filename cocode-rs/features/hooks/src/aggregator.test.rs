use super::*;
use crate::definition::HookHandler;
use crate::event::HookEventType;

fn make_hook(name: &str) -> HookDefinition {
    HookDefinition {
        name: name.to_string(),
        event_type: HookEventType::PreToolUse,
        matcher: None,
        handler: HookHandler::Prompt {
            template: "test".to_string(),
        },
        source: Default::default(),
        enabled: true,
        timeout_secs: 30,
        once: false,
    }
}

#[test]
fn test_empty_aggregator() {
    let aggregator = HookAggregator::new();
    assert!(aggregator.is_empty());
    assert_eq!(aggregator.len(), 0);

    let settings = HookSettings::default();
    let hooks = aggregator.build(&settings);
    assert!(hooks.is_empty());
}

#[test]
fn test_add_policy_hooks() {
    let mut aggregator = HookAggregator::new();
    aggregator.add_policy_hooks(vec![make_hook("p1"), make_hook("p2")]);

    let settings = HookSettings::default();
    let hooks = aggregator.build(&settings);

    assert_eq!(hooks.len(), 2);
    assert!(hooks.iter().all(|h| h.source == HookSource::Policy));
}

#[test]
fn test_add_plugin_hooks() {
    let mut aggregator = HookAggregator::new();
    aggregator.add_plugin_hooks("my-plugin", vec![make_hook("plug1")]);

    let settings = HookSettings::default();
    let hooks = aggregator.build(&settings);

    assert_eq!(hooks.len(), 1);
    assert_eq!(
        hooks[0].source,
        HookSource::Plugin {
            name: "my-plugin".to_string()
        }
    );
}

#[test]
fn test_add_session_hooks() {
    let mut aggregator = HookAggregator::new();
    aggregator.add_session_hooks(vec![make_hook("s1")]);

    let settings = HookSettings::default();
    let hooks = aggregator.build(&settings);

    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks[0].source, HookSource::Session);
}

#[test]
fn test_add_skill_hooks() {
    let mut aggregator = HookAggregator::new();
    aggregator.add_skill_hooks("my-skill", vec![make_hook("sk1")]);

    let settings = HookSettings::default();
    let hooks = aggregator.build(&settings);

    assert_eq!(hooks.len(), 1);
    assert_eq!(
        hooks[0].source,
        HookSource::Skill {
            name: "my-skill".to_string()
        }
    );
}

#[test]
fn test_scope_ordering() {
    let mut aggregator = HookAggregator::new();

    // Add in reverse order
    aggregator.add_skill_hooks("skill", vec![make_hook("sk1")]);
    aggregator.add_session_hooks(vec![make_hook("sess1")]);
    aggregator.add_plugin_hooks("plugin", vec![make_hook("plug1")]);
    aggregator.add_policy_hooks(vec![make_hook("pol1")]);

    let settings = HookSettings::default();
    let hooks = aggregator.build(&settings);

    // Should be sorted by scope priority
    assert_eq!(hooks.len(), 4);
    assert_eq!(hooks[0].source.scope(), HookScope::Policy);
    assert_eq!(hooks[1].source.scope(), HookScope::Plugin);
    assert_eq!(hooks[2].source.scope(), HookScope::Session);
    assert_eq!(hooks[3].source.scope(), HookScope::Skill);
}

#[test]
fn test_managed_hooks_only() {
    let mut aggregator = HookAggregator::new();
    aggregator.add_policy_hooks(vec![make_hook("pol1")]);
    aggregator.add_plugin_hooks("plugin", vec![make_hook("plug1")]);
    aggregator.add_session_hooks(vec![make_hook("sess1")]);
    aggregator.add_skill_hooks("skill", vec![make_hook("sk1")]);

    let settings = HookSettings {
        disable_all_hooks: false,
        allow_managed_hooks_only: true,
    };
    let hooks = aggregator.build(&settings);

    // Only policy and plugin hooks should remain
    assert_eq!(hooks.len(), 2);
    assert!(hooks.iter().all(|h| h.source.is_managed()));
}

#[test]
fn test_disable_all_hooks() {
    let mut aggregator = HookAggregator::new();
    aggregator.add_policy_hooks(vec![make_hook("pol1")]);
    aggregator.add_session_hooks(vec![make_hook("sess1")]);

    let settings = HookSettings {
        disable_all_hooks: true,
        allow_managed_hooks_only: false,
    };
    let hooks = aggregator.build(&settings);

    assert!(hooks.is_empty());
}

#[test]
fn test_aggregate_hooks_helper() {
    let hooks = aggregate_hooks(
        vec![make_hook("pol1")],
        vec![("plugin1".to_string(), vec![make_hook("plug1")])],
        vec![make_hook("sess1")],
        vec![("skill1".to_string(), vec![make_hook("sk1")])],
        &HookSettings::default(),
    );

    assert_eq!(hooks.len(), 4);
    assert_eq!(hooks[0].source.scope(), HookScope::Policy);
    assert_eq!(hooks[1].source.scope(), HookScope::Plugin);
    assert_eq!(hooks[2].source.scope(), HookScope::Session);
    assert_eq!(hooks[3].source.scope(), HookScope::Skill);
}

#[test]
fn test_multiple_hooks_same_scope() {
    let mut aggregator = HookAggregator::new();
    aggregator.add_policy_hooks(vec![make_hook("p1"), make_hook("p2")]);
    aggregator.add_session_hooks(vec![make_hook("s1"), make_hook("s2")]);

    let settings = HookSettings::default();
    let hooks = aggregator.build(&settings);

    assert_eq!(hooks.len(), 4);
    // Policy hooks should come first
    assert_eq!(hooks[0].name, "p1");
    assert_eq!(hooks[1].name, "p2");
    // Session hooks should come after
    assert_eq!(hooks[2].name, "s1");
    assert_eq!(hooks[3].name, "s2");
}
