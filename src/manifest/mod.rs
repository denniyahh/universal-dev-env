use serde::{Deserialize, Serialize};

/// Top-level UDE configuration manifest (.ude.toml or ~/.config/ude/config.toml)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UdeManifest {
    pub environment: EnvironmentConfig,
    pub matrix: MatrixConfig,
    #[serde(default)]
    pub sync: SyncConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnvironmentConfig {
    pub version: String,
    pub profile: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MatrixConfig {
    pub universal: UniversalConfig,
    pub project: ProjectConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UniversalConfig {
    pub generic: bool,
    #[serde(default)]
    pub agents: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectConfig {
    #[serde(rename = "type")]
    pub project_type: String,
    #[serde(default)]
    pub features: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncConfig {
    #[serde(default = "default_strategy")]
    pub strategy: String,
    #[serde(default = "default_true")]
    pub allow_overrides: bool,
    #[serde(default)]
    pub central_repo: Option<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            strategy: default_strategy(),
            allow_overrides: true,
            central_repo: None,
            exclude: vec![
                "*.local.*".to_string(),
                ".ude/overrides/**".to_string(),
                "target/**".to_string(),
                "node_modules/**".to_string(),
            ],
        }
    }
}

fn default_strategy() -> String {
    "three-way".to_string()
}

fn default_true() -> bool {
    true
}

impl Default for UdeManifest {
    fn default() -> Self {
        Self {
            environment: EnvironmentConfig {
                version: "1.0.0".to_string(),
                profile: "default".to_string(),
            },
            matrix: MatrixConfig {
                universal: UniversalConfig {
                    generic: true,
                    agents: vec![
                        "claude".to_string(),
                        "codex".to_string(),
                        "antigravity".to_string(),
                    ],
                },
                project: ProjectConfig {
                    project_type: "rust".to_string(),
                    features: vec![
                        "mempalace".to_string(),
                        "graphify".to_string(),
                        "worktrees".to_string(),
                        "container-parity".to_string(),
                    ],
                },
            },
            sync: SyncConfig {
                strategy: default_strategy(),
                allow_overrides: true,
                central_repo: None,
                exclude: vec![
                    "*.local.*".to_string(),
                    ".ude/overrides/**".to_string(),
                    "target/**".to_string(),
                    "node_modules/**".to_string(),
                ],
            },
        }
    }
}

impl UdeManifest {
    pub fn parse_str(content: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(content)
    }

    pub fn to_toml_string(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_manifest_roundtrip() {
        let manifest = UdeManifest::default();
        let toml_str = manifest.to_toml_string().expect("serialize toml");
        let parsed: UdeManifest = UdeManifest::parse_str(&toml_str).expect("deserialize toml");
        assert_eq!(manifest, parsed);
    }

    #[test]
    fn test_custom_manifest_parsing() {
        let raw = r#"
[environment]
version = "1.0.0"
profile = "fedora-kinoite"

[matrix.universal]
generic = true
agents = ["claude", "antigravity"]

[matrix.project]
type = "node"
features = ["worktrees"]

[sync]
strategy = "three-way"
allow_overrides = true
"#;
        let manifest: UdeManifest = UdeManifest::parse_str(raw).expect("parse custom");
        assert_eq!(manifest.environment.profile, "fedora-kinoite");
        assert_eq!(manifest.matrix.project.project_type, "node");
        assert_eq!(manifest.matrix.universal.agents.len(), 2);
    }
}
