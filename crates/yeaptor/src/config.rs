use anyhow::Result;
use aptos_types::account_address::AccountAddress;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Display;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
// Import IncludedArtifacts from the aptos framework
pub use aptos::move_tool::IncludedArtifacts;
use serde_with::serde_as;

#[derive(Deserialize, Debug, Clone)]
pub struct YeaptorConfig {
    pub format_version: u64,
    pub yeaptor_address: AccountAddress,
    #[serde(default)]
    pub publishers: BTreeMap<String, AccountAddress>,
    #[serde(default, rename = "named-addresses")]
    pub named_addresses: BTreeMap<String, AccountAddress>,
    #[serde(default)]
    pub deployments: Vec<Deployment>,
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeployMethod {
    YeapResourceAccount,
    StandardResourceAccount,
}

impl Default for DeployMethod {
    fn default() -> Self {
        DeployMethod::YeapResourceAccount
    }
}

impl Display for DeployMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeployMethod::YeapResourceAccount => write!(f, "yeap"),
            DeployMethod::StandardResourceAccount => write!(f, "standard"),
        }
    }
}
impl FromStr for DeployMethod {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "yeap" => Ok(DeployMethod::YeapResourceAccount),
            "standard"=> {
                Ok(DeployMethod::StandardResourceAccount)
            }
            _ => Err(format!("Invalid deploy method: {}", s)),
        }
    }
}

impl Serialize for DeployMethod {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for DeployMethod {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct Deployment {
    pub publisher: String,
    pub seed: String,
    #[serde(default)]
    pub method: DeployMethod,
    #[serde(default)]
    pub packages: Vec<PackageSpec>,
}

#[serde_as]
#[derive(Deserialize, Debug, Clone)]
pub struct PackageSpec {
    pub address_name: String,
    pub path: PathBuf,
    #[serde_as(as = "Option<serde_with::DisplayFromStr>")]
    #[serde(default)]
    pub include_artifacts: Option<IncludedArtifacts>,
}

pub fn load_config(path: &Path) -> Result<YeaptorConfig> {
    let s = fs::read_to_string(path)?;
    let cfg: YeaptorConfig = toml::from_str(&s)?;
    Ok(cfg)
}
