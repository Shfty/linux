use anyhow::Result;
use std::path::{Path, PathBuf};

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Fan<P: AsRef<Path>>(pub P);

impl<P: AsRef<Path>> Fan<P> {
    fn suffix(&self, suffix: &str) -> PathBuf {
        let path = self.0.as_ref();
        path.with_file_name(path.file_name().unwrap().to_str().unwrap().to_owned() + suffix)
    }

    pub async fn input(&self) -> Result<u16> {
        let path = self.suffix("_input");
        let s = tokio::fs::read_to_string(path).await?;
        let s = s.strip_suffix('\n').unwrap_or(&s);
        Ok(s.parse().unwrap())
    }
}
