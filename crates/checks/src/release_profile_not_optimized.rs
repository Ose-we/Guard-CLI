//! Detects contract Cargo profiles that are missing size-oriented release settings.

use crate::util::is_cfg_test;
use crate::{Check, CheckContext, Finding, Severity};
use std::collections::HashSet;
use syn::spanned::Spanned;
use syn::{File, Item};

const CHECK_NAME: &str = "release-profile-not-optimized";
const RULE_URL: &str =
    "https://github.com/SorobanGuard/Guard-CLI/blob/main/docs/checks.md#release-profile-not-optimized";

const REQUIRED_SETTINGS: &[(&str, &str)] = &[
    ("opt-level", "\"z\""),
    ("lto", "true"),
    ("panic", "\"abort\""),
    ("codegen-units", "1"),
    ("strip", "\"symbols\""),
];

const BUILT_IN_PROFILES: &[&str] = &["dev", "release", "test", "bench", "build-override"];

pub struct ReleaseProfileNotOptimizedCheck;

impl Check for ReleaseProfileNotOptimizedCheck {
    fn name(&self) -> &str {
        CHECK_NAME
    }

    fn needs_context(&self, file: &File) -> bool {
        contract_annotation_line(&file.items).is_some()
    }

    fn run(&self, _file: &File, _source: &str) -> Vec<Finding> {
        Vec::new()
    }

    fn run_with_context(
        &self,
        file: &File,
        _source: &str,
        context: &CheckContext<'_>,
    ) -> Vec<Finding> {
        let Some(line) = contract_annotation_line(&file.items) else {
            return Vec::new();
        };
        let Some(manifest) = context.workspace_manifest.or(context.manifest) else {
            return Vec::new();
        };
        let Some(profiles) = manifest
            .get("profile")
            .and_then(toml::Value::as_table)
        else {
            return findings_for_profile("release", None, line);
        };

        let mut findings = findings_for_profile("release", Some(profiles), line);
        for (profile_name, value) in profiles {
            if BUILT_IN_PROFILES.contains(&profile_name.as_str())
                || !value.is_table()
            {
                continue;
            }
            findings.extend(findings_for_profile(
                profile_name,
                Some(profiles),
                line,
            ));
        }
        findings
    }
}

fn contract_annotation_line(items: &[Item]) -> Option<usize> {
    for item in items {
        match item {
            Item::Mod(module) => {
                let is_test = is_cfg_test(&module.attrs)
                    || module.ident == "test"
                    || module.ident == "tests";
                if !is_test {
                    if let Some((_, nested)) = &module.content {
                        if let Some(line) = contract_annotation_line(nested) {
                            return Some(line);
                        }
                    }
                }
            }
            Item::Struct(item_struct) if !is_cfg_test(&item_struct.attrs) => {
                if item_struct
                    .attrs
                    .iter()
                    .any(|attr| attr.path().is_ident("contract"))
                {
                    return Some(item_struct.ident.span().start().line);
                }
            }
            Item::Impl(item_impl) if !is_cfg_test(&item_impl.attrs) => {
                if item_impl
                    .attrs
                    .iter()
                    .any(|attr| attr.path().is_ident("contractimpl"))
                {
                    return Some(item_impl.self_ty.span().start().line);
                }
            }
            _ => {}
        }
    }
    None
}

fn findings_for_profile(
    profile_name: &str,
    profiles: Option<&toml::map::Map<String, toml::Value>>,
    line: usize,
) -> Vec<Finding> {
    REQUIRED_SETTINGS
        .iter()
        .filter_map(|(key, expected)| {
            let actual = profiles.and_then(|profiles| {
                profile_setting(profiles, profile_name, key, &mut HashSet::new())
            });
            if actual.is_some_and(|value| setting_matches(key, value)) {
                return None;
            }
            Some(Finding {
                check_name: CHECK_NAME.to_string(),
                severity: Severity::Low,
                file_path: String::new(),
                line,
                function_name: String::new(),
                description: format!(
                    "Cargo profile `{profile_name}` is missing or overrides `{key}`; \
                     this can increase the deployed Soroban WASM size."
                ),
                rule_url: Some(RULE_URL.to_string()),
                suggestion: Some(format!(
                    "Add this to Cargo.toml:\n[profile.{profile_name}]\n{key} = {expected}"
                )),
            })
        })
        .collect()
}

fn profile_setting<'a>(
    profiles: &'a toml::map::Map<String, toml::Value>,
    profile_name: &str,
    key: &str,
    visited: &mut HashSet<String>,
) -> Option<&'a toml::Value> {
    if !visited.insert(profile_name.to_string()) {
        return None;
    }
    let profile = profiles.get(profile_name)?.as_table()?;
    if let Some(value) = profile.get(key) {
        return Some(value);
    }
    profile
        .get("inherits")
        .and_then(toml::Value::as_str)
        .and_then(|parent| profile_setting(profiles, parent, key, visited))
}

fn setting_matches(key: &str, value: &toml::Value) -> bool {
    match key {
        "opt-level" => value.as_str() == Some("z"),
        "lto" => value.as_bool() == Some(true),
        "panic" => value.as_str() == Some("abort"),
        "codegen-units" => value.as_integer() == Some(1),
        "strip" => value.as_str() == Some("symbols"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Check;

    fn contract_file(source: &str) -> File {
        syn::parse_file(source).expect("contract source should parse")
    }

    fn parsed_manifest(source: &str) -> toml::Value {
        source.parse().expect("manifest should parse")
    }

    fn context<'a>(manifest: &'a toml::Value) -> CheckContext<'a> {
        CheckContext {
            manifest: Some(manifest),
            workspace_manifest: None,
        }
    }

    const CONTRACT: &str = "#[contract]\npub struct C;\n";

    #[test]
    fn reports_each_missing_release_setting_with_remediation() {
        let manifest = parsed_manifest("[package]\nname = \"contract\"\n");
        let findings = ReleaseProfileNotOptimizedCheck.run_with_context(
            &contract_file(CONTRACT),
            CONTRACT,
            &context(&manifest),
        );

        assert_eq!(findings.len(), REQUIRED_SETTINGS.len());
        assert!(findings.iter().all(|finding| {
            finding.severity == Severity::Low
                && finding.rule_url.as_deref() == Some(RULE_URL)
                && finding.suggestion.as_deref().is_some_and(|suggestion| {
                    suggestion.contains("[profile.release]")
                })
        }));
    }

    #[test]
    fn accepts_the_complete_release_profile() {
        let manifest = parsed_manifest(
            r#"
[profile.release]
opt-level = "z"
lto = true
panic = "abort"
codegen-units = 1
strip = "symbols"
"#,
        );
        let findings = ReleaseProfileNotOptimizedCheck.run_with_context(
            &contract_file(CONTRACT),
            CONTRACT,
            &context(&manifest),
        );

        assert!(findings.is_empty());
    }

    #[test]
    fn ignores_contract_annotations_gated_to_tests() {
        let manifest = parsed_manifest("[profile.release]\n");
        let source = "#[cfg(test)]\n#[contract]\npub struct TestOnly;\n";
        let findings = ReleaseProfileNotOptimizedCheck.run_with_context(
            &contract_file(source),
            source,
            &context(&manifest),
        );

        assert!(findings.is_empty());
    }

    #[test]
    fn ignores_rust_files_that_do_not_contain_a_contract() {
        let manifest = parsed_manifest("[profile.release]\n");
        let source = "pub struct OrdinaryLibrary;\n";
        let findings = ReleaseProfileNotOptimizedCheck.run_with_context(
            &contract_file(source),
            source,
            &context(&manifest),
        );

        assert!(findings.is_empty());
    }

    #[test]
    fn uses_workspace_profile_settings_for_member_contracts() {
        let member = parsed_manifest("[package]\nname = \"contract\"\n");
        let workspace = parsed_manifest(
            r#"
[workspace]
members = ["contract"]

[profile.release]
opt-level = "z"
lto = true
panic = "abort"
codegen-units = 1
strip = "symbols"
"#,
        );
        let context = CheckContext {
            manifest: Some(&member),
            workspace_manifest: Some(&workspace),
        };
        let findings = ReleaseProfileNotOptimizedCheck.run_with_context(
            &contract_file(CONTRACT),
            CONTRACT,
            &context,
        );

        assert!(findings.is_empty());
    }

    #[test]
    fn checks_custom_profiles_and_respects_release_inheritance() {
        let manifest = parsed_manifest(
            r#"
[profile.release]
opt-level = "z"
lto = true
panic = "abort"
codegen-units = 1
strip = "symbols"

[profile.release-with-logs]
inherits = "release"
lto = false
"#,
        );
        let findings = ReleaseProfileNotOptimizedCheck.run_with_context(
            &contract_file(CONTRACT),
            CONTRACT,
            &context(&manifest),
        );

        assert_eq!(findings.len(), 1);
        assert!(findings[0].description.contains("release-with-logs"));
        assert!(findings[0].suggestion.as_deref().unwrap().contains("[profile.release-with-logs]"));
    }
}
