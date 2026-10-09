use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemOneRequest {
    pub state: serde_json::Value,
    pub model: String,
    pub questions: HashMap<String, Question>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Question {
    #[serde(rename = "noul")]
    Noul {
        instructions: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        criteria: Option<NoulCriteria>,
    },
    #[serde(rename = "choice")]
    Choice {
        instructions: serde_json::Value,
        criteria: HashMap<String, serde_json::Value>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoulCriteria {
    #[serde(rename = "true", skip_serializing_if = "Option::is_none")]
    pub true_val: Option<String>,
    #[serde(rename = "false", skip_serializing_if = "Option::is_none")]
    pub false_val: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemOneResponse {
    pub model: String,
    pub answers: HashMap<String, Answer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Answer {
    #[serde(rename = "noul")]
    Noul { noul: f64 },
    #[serde(rename = "choice")]
    Choice {
        choice: String,
        probabilities: HashMap<String, f64>,
        confidence: f64,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum SemanticDriftVerdict {
    BenignRephrase,
    ProjectException,
    PolicyShift,
    Uncertain(String),
}

#[derive(Default)]
pub struct TypeSafeClient {
    api_key: Option<String>,
}

impl TypeSafeClient {
    pub fn new() -> Self {
        let api_key = std::env::var("TYPESAFE_API_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty());
        Self { api_key }
    }

    pub fn is_available(&self) -> bool {
        self.api_key.is_some()
    }

    /// Evaluates a semantic drift between upstream original rule and local candidate update
    pub fn evaluate_drift(
        &self,
        rule_name: &str,
        original_text: &str,
        candidate_text: &str,
    ) -> Result<SemanticDriftVerdict, String> {
        let api_key = match &self.api_key {
            Some(k) => k,
            None => return Err("TYPESAFE_API_KEY is not set in environment".to_string()),
        };

        let mut criteria = HashMap::new();
        criteria.insert(
            "benign_rephrase".to_string(),
            serde_json::json!("Typo fixes, grammar, styling, or clearer phrasing with identical governance intent"),
        );
        criteria.insert(
            "project_exception".to_string(),
            serde_json::json!("Adds exceptions, paths, or tool quirks specific to one repository rather than universal"),
        );
        criteria.insert(
            "policy_shift".to_string(),
            serde_json::json!("Weakens, removes, or changes core rules (e.g. RULE ZERO verification standards, negative control requirements)"),
        );

        let mut questions = HashMap::new();
        questions.insert(
            "drift_classification".to_string(),
            Question::Choice {
                instructions: serde_json::json!(format!(
                    "Classify how this proposed change modifies the core governance rule '{rule_name}'."
                )),
                criteria,
            },
        );

        let request = SystemOneRequest {
            state: serde_json::json!({
                "rule_name": rule_name,
                "original_rule": original_text,
                "proposed_update": candidate_text,
            }),
            model: "jev-latest".to_string(),
            questions,
        };

        let req_json = serde_json::to_string(&request)
            .map_err(|e| format!("Failed to serialize request: {e}"))?;

        let mut child = Command::new("curl")
            .args([
                "-s",
                "--connect-timeout",
                "5",
                "--max-time",
                "15",
                "-X",
                "POST",
                "https://api.typesafe.ai/v1/systemone",
                "-H",
                &format!("Authorization: Bearer {api_key}"),
                "-H",
                "Content-Type: application/json",
                "--data-binary",
                "@-",
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn curl: {e}"))?;

        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(req_json.as_bytes());
        }

        let output = child
            .wait_with_output()
            .map_err(|e| format!("curl execution failed: {e}"))?;

        if !output.status.success() {
            return Err("HTTP request failed or timed out".to_string());
        }

        let resp_str = String::from_utf8_lossy(&output.stdout);
        let resp: SystemOneResponse = serde_json::from_str(&resp_str)
            .map_err(|e| format!("Failed to parse TypeSafe response: {e}; body: {resp_str}"))?;

        if let Some(Answer::Choice { choice, .. }) = resp.answers.get("drift_classification") {
            match choice.as_str() {
                "benign_rephrase" => Ok(SemanticDriftVerdict::BenignRephrase),
                "project_exception" => Ok(SemanticDriftVerdict::ProjectException),
                "policy_shift" => Ok(SemanticDriftVerdict::PolicyShift),
                other => Ok(SemanticDriftVerdict::Uncertain(other.to_string())),
            }
        } else {
            Err("No drift_classification answer in response".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_typesafe_request_serialization() {
        let mut criteria = HashMap::new();
        criteria.insert("true".to_string(), serde_json::json!("High risk of credential leak"));
        criteria.insert("false".to_string(), serde_json::json!("Benign text"));

        let mut questions = HashMap::new();
        questions.insert(
            "is_secret".to_string(),
            Question::Noul {
                instructions: serde_json::json!("Does this contain an active secret?"),
                criteria: Some(NoulCriteria {
                    true_val: Some("High risk".to_string()),
                    false_val: Some("Benign".to_string()),
                }),
            },
        );

        let req = SystemOneRequest {
            state: serde_json::json!({"code": "const api_key = 'test';"}),
            model: "jev-latest".to_string(),
            questions,
        };

        let serialized = serde_json::to_string(&req).expect("serialize");
        assert!(serialized.contains("jev-latest"));
        assert!(serialized.contains("is_secret"));
    }

    #[test]
    fn test_typesafe_response_deserialization() {
        let json = r#"{
            "model": "jev-1.13.0",
            "answers": {
                "drift_classification": {
                    "type": "choice",
                    "choice": "benign_rephrase",
                    "probabilities": { "benign_rephrase": 0.92, "policy_shift": 0.08 },
                    "confidence": 0.89
                }
            }
        }"#;

        let resp: SystemOneResponse = serde_json::from_str(json).expect("deserialize");
        assert_eq!(resp.model, "jev-1.13.0");
        if let Some(Answer::Choice { choice, confidence, .. }) = resp.answers.get("drift_classification") {
            assert_eq!(choice, "benign_rephrase");
            assert!(*confidence > 0.8);
        } else {
            panic!("Expected Choice answer");
        }
    }
}
