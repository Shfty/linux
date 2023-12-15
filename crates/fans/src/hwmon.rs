use futures::StreamExt;
use std::{collections::BTreeMap, fmt::Display, ops::Deref};

use crate::HwmonEntry;
use anyhow::Result;

#[derive(Debug, Default, Clone)]
pub struct Hwmon(BTreeMap<String, HwmonEntry>);

impl Deref for Hwmon {
    type Target = BTreeMap<String, HwmonEntry>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Display for Hwmon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Hwmon\n")?;
        for (name, entry) in &self.0 {
            f.write_fmt(format_args!("\t{}\n", &name))?;

            let runtime = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
            let _guard = runtime.enter();

            if entry.fans.len() > 0 {
                f.write_str("\t\tFans\n")?;

                for (i, fan) in entry.fans.iter() {
                    f.write_fmt(format_args!(
                        "\t\tFan {i:}: {}RPM\n",
                        runtime
                            .block_on(fan.input())
                            .expect("Failed to read fan input")
                    ))?;
                }

                f.write_str("\n")?;
            }

            if entry.pwms.len() > 0 {
                f.write_str("\t\tPWMs\n")?;
                for (i, pwm) in entry.pwms.iter() {
                    f.write_fmt(format_args!(
                        "\t\tPWM {i:}: {}\n",
                        runtime
                            .block_on(pwm.get_value())
                            .expect("Failed to read PWM input")
                    ))?;
                }

                f.write_str("\n")?;
            }

            if entry.temps.len() > 0 {
                f.write_str("\t\tTemps\n")?;
                for (i, temp) in entry.temps.iter() {
                    f.write_fmt(format_args!(
                        "\t\t{i:}: {}: {}C\n",
                        runtime
                            .block_on(temp.label())
                            .expect("Failed to read temp label"),
                        runtime
                            .block_on(temp.input())
                            .expect("Failed to read temp input") as f32
                            / 1000.0
                    ))?;
                }
            }

            f.write_str("\n")?;
        }

        Ok(())
    }
}

impl Hwmon {
    pub async fn new() -> Result<Hwmon> {
        let mut entries = BTreeMap::default();

        let mut dir = tokio_stream::wrappers::ReadDirStream::new(
            tokio::fs::read_dir("/sys/class/hwmon").await?,
        );
        while let Some(entry) = dir.next().await {
            let hwmon_entry = HwmonEntry::new(entry?.path()).await?;
            if hwmon_entry.is_empty() {
                continue;
            }
            entries.insert(hwmon_entry.name.to_owned(), hwmon_entry);
        }

        Ok(Hwmon(entries))
    }
}
