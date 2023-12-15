use serde::Deserialize;
use std::{
    error::Error,
    ffi::OsStr,
    ops::{Deref, DerefMut},
    path::Path,
};
use toml::Value;

#[derive(Debug, Clone, Deserialize)]
pub struct Commands {
    pub build: Option<String>,
    pub test: Option<String>,
    pub run: Option<String>,
    pub deploy: Option<String>,
    pub debug: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Predicate {
    pub file: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Workflow {
    pub name: String,
    pub icon: Option<String>,
    pub predicate: Option<Predicate>,
    pub commands: Option<Commands>,
}

#[derive(Debug, Clone)]
pub struct Workflows(Vec<Workflow>);

impl Deref for Workflows {
    type Target = Vec<Workflow>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Workflows {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Into<Vec<Workflow>> for Workflows {
    fn into(self) -> Vec<Workflow> {
        self.0
    }
}

impl Workflows {
    pub fn new<P: AsRef<Path>>(dir: P) -> Result<Self, Box<dyn Error>> {
        let workflow_extension = OsStr::new("workflow");

        let mut workflows = vec![];
        for result in std::fs::read_dir(dir)? {
            let result = result?;
            if !result.file_type()?.is_file() {
                continue;
            }

            let path = result.path();
            if path.extension() != Some(&workflow_extension) {
                continue;
            }

            let config = std::fs::read_to_string(path)?;
            let toml = config.parse::<Value>()?;
            workflows.push(toml.try_into::<Workflow>()?);
        }

        Ok(Workflows(workflows))
    }

    pub fn workflow(&self, workspace_path: &str) -> Option<&Workflow> {
        if let Some(workflow) = self.iter().find(|workflow| {
            if let Some(predicate) = &workflow.predicate {
                if let Some(path) = &predicate.path {
                    path == workspace_path
                } else {
                    false
                }
            } else {
                false
            }
        }) {
            Some(workflow)
        } else if let Some(workflow) = self.iter().find(|workflow| {
            if let Some(predicate) = &workflow.predicate {
                if let Some(file) = &predicate.file {
                    std::fs::read_dir(&workspace_path)
                        .unwrap()
                        .any(|entry| entry.unwrap().file_name() == OsStr::new(&file))
                } else {
                    false
                }
            } else {
                false
            }
        }) {
            Some(workflow)
        } else {
            None
        }
    }
}
