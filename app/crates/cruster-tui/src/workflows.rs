//! Saved investigative workflows loaded from
//! ~/.config/cruster/workflows/*.toml.

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Workflow {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Deserialize)]
pub enum Step {
    #[serde(rename = "switch_view")]
    SwitchView(String),
    #[serde(rename = "set_filter")]
    SetFilter(String),
}

pub fn load_all() -> Vec<Workflow> {
    let Some(dir) = workflows_dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut workflows = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        if let Ok(body) = std::fs::read_to_string(&path) {
            if let Ok(w) = toml::from_str::<Workflow>(&body) {
                workflows.push(w);
            }
        }
    }
    workflows
}

fn workflows_dir() -> Option<PathBuf> {
    let mut p = dirs::config_dir()?;
    p.push("cruster");
    p.push("workflows");
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_workflow() {
        let body = r#"
            name = "Stuck rollouts"
            description = "Find deployments not fully available"
            steps = [
              { switch_view = "deployments" },
              { set_filter = "status:Available" },
            ]
        "#;
        let w: Workflow = toml::from_str(body).unwrap();
        assert_eq!(w.name, "Stuck rollouts");
        assert_eq!(w.steps.len(), 2);
        assert!(matches!(&w.steps[0], Step::SwitchView(s) if s == "deployments"));
    }
}
