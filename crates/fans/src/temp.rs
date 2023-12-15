use anyhow::Result;
use std::path::{Path, PathBuf};

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Temp<P: AsRef<Path>>(pub P);

impl<P: AsRef<Path>> Temp<P> {
    fn suffix(&self, suffix: &str) -> PathBuf {
        let path = self.0.as_ref();
        path.with_file_name(path.file_name().unwrap().to_str().unwrap().to_owned() + suffix)
    }

    pub async fn label(&self) -> Result<String> {
        let path = self.suffix("_label");
        let s = tokio::fs::read_to_string(path).await?;
        let s = s.strip_suffix('\n').unwrap_or(&s);
        Ok(s.to_owned())
    }

    pub async fn input(&self) -> Result<i32> {
        let path = self.suffix("_input");
        let s = tokio::fs::read_to_string(path).await?;
        let s = s.strip_suffix('\n').unwrap_or(&s);
        Ok(s.parse().unwrap())
    }
}
