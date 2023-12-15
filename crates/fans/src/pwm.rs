use anyhow::{anyhow, Error, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum AmdGpuPwmEnable {
    Disabled,
    Manual,
    Automatic,
}

impl TryFrom<u8> for AmdGpuPwmEnable {
    type Error = Error;

    fn try_from(val: u8) -> std::result::Result<Self, Self::Error> {
        Ok(match val {
            0 => AmdGpuPwmEnable::Disabled,
            1 => AmdGpuPwmEnable::Manual,
            2 => AmdGpuPwmEnable::Automatic,
            _ => return Err(anyhow!("Invalid AmdGpuPwmEnable")),
        })
    }
}

impl From<AmdGpuPwmEnable> for u8 {
    fn from(enable: AmdGpuPwmEnable) -> Self {
        match enable {
            AmdGpuPwmEnable::Disabled => 0,
            AmdGpuPwmEnable::Manual => 1,
            AmdGpuPwmEnable::Automatic => 2,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum MsiPwmEnable {
    Disabled,
    Manual,
    ThermalCruise,
    SpeedCruise,
    SmartFanIII,
    SmartFanIV,
}

impl TryFrom<u8> for MsiPwmEnable {
    type Error = Error;

    fn try_from(val: u8) -> Result<Self, Self::Error> {
        Ok(match val {
            0 => MsiPwmEnable::Disabled,
            1 => MsiPwmEnable::Manual,
            2 => MsiPwmEnable::ThermalCruise,
            3 => MsiPwmEnable::SpeedCruise,
            4 => MsiPwmEnable::SmartFanIII,
            5 => MsiPwmEnable::SmartFanIV,
            _ => return Err(anyhow!("Invalid MsiPwmEnable")),
        })
    }
}

impl From<MsiPwmEnable> for u8 {
    fn from(enable: MsiPwmEnable) -> Self {
        match enable {
            MsiPwmEnable::Disabled => 0,
            MsiPwmEnable::Manual => 1,
            MsiPwmEnable::ThermalCruise => 2,
            MsiPwmEnable::SpeedCruise => 3,
            MsiPwmEnable::SmartFanIII => 4,
            MsiPwmEnable::SmartFanIV => 5,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum PwmMode {
    DC,
    PWM,
}

impl TryFrom<u8> for PwmMode {
    type Error = Error;

    fn try_from(val: u8) -> Result<Self, Self::Error> {
        Ok(match val {
            0 => PwmMode::DC,
            1 => PwmMode::PWM,
            _ => return Err(anyhow!("Invalid PwmMode")),
        })
    }
}

impl From<PwmMode> for u8 {
    fn from(mode: PwmMode) -> Self {
        match mode {
            PwmMode::DC => 0,
            PwmMode::PWM => 1,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pwm<P: AsRef<Path>>(pub P);

impl<P: AsRef<Path>> Pwm<P> {
    fn suffix(&self, suffix: &str) -> PathBuf {
        let path = self.0.as_ref();
        path.with_file_name(path.file_name().unwrap().to_str().unwrap().to_owned() + suffix)
    }

    pub async fn get_value(&self) -> Result<u8> {
        let s = tokio::fs::read_to_string(&self.0).await?;
        let s = s.strip_suffix('\n').unwrap_or(&s);
        Ok(s.parse().unwrap())
    }

    pub async fn set_value(&self, value: u8) -> Result<()> {
        Ok(tokio::fs::write(&self.0, value.to_string()).await?)
    }

    pub async fn get_enable(&self) -> Result<u8> {
        let s = tokio::fs::read_to_string(self.suffix("_enable")).await?;
        let s = s.strip_suffix('\n').unwrap_or(&s);
        Ok(s.parse::<u8>().unwrap())
    }

    pub async fn set_enable(&self, enable: u8) -> Result<()> {
        Ok(tokio::fs::write(self.suffix("_enable"), u8::from(enable).to_string()).await?)
    }

    pub async fn get_mode(&self) -> Result<PwmMode> {
        let s = tokio::fs::read_to_string(self.suffix("_mode")).await?;
        let s = s.strip_suffix('\n').unwrap_or(&s);
        s.parse::<u8>().unwrap().try_into()
    }

    pub async fn set_mode(&self, mode: PwmMode) -> Result<()> {
        Ok(tokio::fs::write(
            self.suffix("_enable"),
            u8::from(mode).to_string(),
        ).await?)
    }
}
