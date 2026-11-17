use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub const STAGES: &[&str] = &[
    "initialize",
    "design",
    "coding",
    "debugging",
    "verification",
    "tools",
];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Applicability {
    #[serde(default)]
    pub core: bool,
    #[serde(default)]
    pub projects: Vec<String>,
    #[serde(default)]
    pub technologies: Vec<String>,
    #[serde(default)]
    pub stages: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecallSelection {
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub technologies: Vec<String>,
    #[serde(default)]
    pub stage: Option<String>,
}

fn validate_values(values: &[String]) -> Result<()> {
    if values.len() > 32
        || values
            .iter()
            .any(|value| value.trim().is_empty() || value.len() > 300 || value.contains('\0'))
    {
        return Err(Error::InvalidValue {
            field: "applicability",
            value: "use up to 32 nonempty selectors, each at most 300 bytes".into(),
        });
    }
    Ok(())
}

impl Applicability {
    pub fn validate(&self) -> Result<()> {
        validate_values(&self.projects)?;
        validate_values(&self.technologies)?;
        validate_values(&self.stages)?;
        if self
            .stages
            .iter()
            .any(|stage| !STAGES.contains(&stage.as_str()))
        {
            return Err(Error::InvalidValue {
                field: "stages",
                value: format!("choose from {}", STAGES.join(", ")),
            });
        }
        Ok(())
    }

    pub fn matches(&self, selection: &RecallSelection) -> bool {
        (self.projects.is_empty()
            || selection
                .project
                .as_ref()
                .is_some_and(|project| self.projects.contains(project)))
            && (self.stages.is_empty()
                || selection
                    .stage
                    .as_ref()
                    .is_some_and(|stage| self.stages.contains(stage)))
            && (self.technologies.is_empty()
                || self.technologies.iter().any(|technology| {
                    selection
                        .technologies
                        .iter()
                        .any(|actual| actual.eq_ignore_ascii_case(technology))
                }))
    }
}

impl RecallSelection {
    pub fn validate(&self) -> Result<()> {
        Applicability {
            projects: self.project.iter().cloned().collect(),
            technologies: self.technologies.clone(),
            stages: self.stage.iter().cloned().collect(),
            ..Default::default()
        }
        .validate()
    }
}

// Applied inside SQL before ranking limits; traversal uses the same matching semantics.
pub(crate) const APPLICABLE_SQL: &str = "((json_extract(n.applicability, '$.core') IS NOT 1 OR n.provenance_source != 'model') AND (n.applicability IS NULL OR (
    (json_array_length(n.applicability, '$.projects') = 0 OR EXISTS
        (SELECT 1 FROM json_each(n.applicability, '$.projects') p WHERE p.value = json_extract(?3, '$.project')))
    AND (json_array_length(n.applicability, '$.stages') = 0 OR EXISTS
        (SELECT 1 FROM json_each(n.applicability, '$.stages') s WHERE s.value = json_extract(?3, '$.stage')))
    AND (json_array_length(n.applicability, '$.technologies') = 0 OR EXISTS
        (SELECT 1 FROM json_each(n.applicability, '$.technologies') t
         JOIN json_each(?3, '$.technologies') actual ON lower(t.value) = lower(actual.value)))
)))";
