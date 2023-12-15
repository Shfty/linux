use crate::{Fan, Pwm, Temp, Then};
use anyhow::{anyhow, Result};
use futures::StreamExt;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct HwmonEntry {
    pub name: String,
    pub fans: BTreeMap<usize, Fan<PathBuf>>,
    pub pwms: BTreeMap<usize, Pwm<PathBuf>>,
    pub temps: BTreeMap<usize, Temp<PathBuf>>,
}

impl HwmonEntry {
    pub async fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();

        let mut name = None;

        let mut pwms = BTreeMap::default();
        let mut fans = BTreeMap::default();
        let mut temps = BTreeMap::default();

        let mut dir = tokio_stream::wrappers::ReadDirStream::new(tokio::fs::read_dir(path).await?);
        while let Some(entry) = dir.next().await {
            let entry = entry?;

            let file_name = entry.file_name();

            let file_name = file_name.to_str().ok_or(anyhow!("Invalid filename"))?;

            if file_name == "name" {
                let s = tokio::fs::read_to_string(entry.path()).await?;
                let s = s.strip_suffix('\n').unwrap_or(&s).to_string();
                name = Some(s);
            }
            if file_name.len() == 4 && file_name.starts_with("pwm") {
                let idx = file_name
                    .chars()
                    .nth(3)
                    .ok_or(anyhow!("Invalid PWM"))?
                    .then(|c| c.to_string())
                    .then(|s| s.parse::<usize>())
                    .unwrap();

                pwms.insert(idx, Pwm(path.join(file_name)));
            } else if file_name.starts_with("fan") && file_name.ends_with("_input") {
                let idx = file_name
                    .chars()
                    .nth(3)
                    .ok_or(anyhow!("Invalid fan"))?
                    .then(|c| c.to_string())
                    .then(|s| s.parse::<usize>())
                    .unwrap();

                fans.insert(
                    idx,
                    Fan(path.join(file_name.strip_suffix("_input").unwrap_or(&file_name))),
                );
            } else if file_name.starts_with("temp") && file_name.ends_with("_input") {
                let idx = file_name
                    .chars()
                    .nth(4)
                    .ok_or(anyhow!("Invalid temp"))?
                    .then(|c| c.to_string())
                    .then(|s| s.parse::<usize>())
                    .unwrap();

                temps.insert(
                    idx,
                    Temp(path.join(file_name.strip_suffix("_input").unwrap_or(&file_name))),
                );
            }
        }

        let name = name.expect("Hwmon entry has no name");

        Ok(HwmonEntry {
            name,
            fans,
            pwms,
            temps,
        })
    }
    pub fn is_empty(&self) -> bool {
        self.fans.is_empty() && self.pwms.is_empty() && self.temps.is_empty()
    }
}
