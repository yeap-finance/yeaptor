use crate::config::YeaptorConfig;
use anyhow::anyhow;

use aptos::common::types::{CliError, CliTypedResult, MovePackageOptions};
use aptos::move_tool::{IncludedArtifacts, IncludedArtifactsArgs};
use aptos_framework::BuiltPackage;
use aptos_types::account_address::{AccountAddress, create_resource_address};
use std::collections::BTreeMap;

use std::path::Path;
use aptos_framework::docgen::DocgenOptions;
use move_binary_format::access::ModuleAccess;

#[derive(Debug, Clone)]
pub struct YeaptorEnv {
    config: YeaptorConfig,
    named_addresses: BTreeMap<String, AccountAddress>,
    package_addresses: BTreeMap<String, AccountAddress>,
}
pub struct BuiltDeployment {
    #[allow(unused)]
    pub publisher: AccountAddress,
    pub seed: String,
    pub package_address: Option<AccountAddress>,
    pub pack: BuiltPackage, // (package_name, metadata_serialized, modules)
}

fn domain_separated_seed(ra_address: &AccountAddress, mut seed: Vec<u8>) -> Vec<u8> {
    let mut final_seed = bcs::to_bytes(ra_address).unwrap();
    final_seed.append(&mut "::ra_code_deployment::".as_bytes().to_vec());
    final_seed.append(&mut seed);
    final_seed
}
impl YeaptorEnv {
    pub fn new(config: YeaptorConfig) -> Self {

        let package_addresses = config
            .deployments
            .iter()
            .flat_map(|de| {
                let deployment_address = create_resource_address(
                    config
                        .publishers
                        .get(de.publisher.as_str())
                        .unwrap()
                        .clone(),
                    &domain_separated_seed(&config.yeaptor_address, de.seed.as_bytes().to_vec()),
                );
                de.packages
                    .iter()
                    .map(move |package| (package.address_name.clone(), deployment_address.clone()))
            })
            .collect::<BTreeMap<String, AccountAddress>>();
        // if user overrides package addresses in named addresses config, use those in preference
        let named_addresses: BTreeMap<_, _> = config.named_addresses.clone();

        Self {
            config,
            named_addresses,
            package_addresses,
        }
    }
    pub fn config(&self) -> &YeaptorConfig {
        &self.config
    }

    pub fn deploy_order(&self, package_path: &Path) -> CliTypedResult<Option<u64>> {
        let package_path = package_path.canonicalize().map_err(|e| {
            CliError::IO(
                format!(
                    "Failed to canonicalize package path {}",
                    package_path.display()
                ),
                e,
            )
        })?;
        let mut i = 0;
        for d in &self.config.deployments {
            for p in &d.packages {
                let path = p.path.canonicalize().map_err(|e| {
                    CliError::IO(
                        format!("Failed to canonicalize package path {}", p.path.display()),
                        e,
                    )
                })?;
                if path == package_path {
                    return Ok(Some(i));
                }
                i += 1;
            }
        }
        Ok(None)
    }
    #[allow(unused)]
    pub fn named_addresses(&self) -> &BTreeMap<String, AccountAddress> {
        &self.named_addresses
    }

    pub fn is_package_address_overridden(&self, package_name: &str) -> bool {
        self.named_addresses.contains_key(package_name)
    }

    pub fn build_all(
        &self,
        included_args: &IncludedArtifactsArgs,
        move_options: &MovePackageOptions,
        docgen_options: Option<DocgenOptions>,
    ) -> CliTypedResult<Vec<BuiltDeployment>> {
        let mut deployments = Vec::new();
        for deployment in &self.config.deployments {
            let publisher = self
                .config
                .publishers
                .get(&deployment.publisher)
                .expect(&format!(
                    "Publisher address not found: {}",
                    deployment.publisher
                ))
                .clone();
            let seed = deployment.seed.clone();
            for pkg in &deployment.packages {
                let pkg_path = Path::new(&pkg.path);
                let included_artifacts = pkg
                    .include_artifacts
                    .as_ref()
                    .unwrap_or(&included_args.included_artifacts);
                let pack = self
                    .build_package(pkg_path, included_artifacts, move_options, docgen_options.clone())
                    .expect("Failed to build package");

                let existing_package_address = if self.is_package_address_overridden(pkg.address_name.as_str()) {
                    let package_address = pack.modules().map(|m| m.address()).next().unwrap();
                    assert!(self.named_addresses.get(pkg.address_name.as_str()).unwrap() == package_address);
                    Some(*package_address)
                } else {
                    None
                };
                let d = BuiltDeployment {
                    package_address: existing_package_address,
                    publisher: publisher.clone(),
                    seed: seed.clone(),
                    pack,
                };
                deployments.push(d);
            }
        }
        Ok(deployments)
    }

    pub fn build_package(
        &self,
        package_dir: &Path,
        included_args: &IncludedArtifacts,
        move_options: &MovePackageOptions,
        docgen_options: Option<DocgenOptions>,
    ) -> CliTypedResult<BuiltPackage> {
        let mut build_options = included_args.build_options(move_options)?;
        build_options.install_dir = move_options.output_dir.clone();

        // Merge named addresses: package addresses < env named addresses < user named addresses
        let mut named_addresses = self.package_addresses.clone();
        named_addresses.extend(self.named_addresses.clone());
        named_addresses.extend(build_options.named_addresses.clone());
        build_options.named_addresses = named_addresses;
        build_options.with_docs = docgen_options.is_some();
        build_options.docgen_options = docgen_options;
        let pack = BuiltPackage::build(package_dir.to_path_buf(), build_options)
            .map_err(|e| anyhow!("Move compilation error: {:#}", e))?;
        Ok(pack)
    }

    pub fn build_deployment_package(
        &self,
        package_dir: &Path,
        included_args: &IncludedArtifactsArgs,
        move_options: &MovePackageOptions,
        doc_options: Option<DocgenOptions>,
    ) -> CliTypedResult<(usize, BuiltDeployment)> {
        // Canonicalize the input package directory for proper comparison
        let canonical_package_dir = package_dir.canonicalize().map_err(|e| {
            CliError::IO(
                format!(
                    "Failed to canonicalize package directory {}",
                    package_dir.display(),
                ),
                e,
            )
        })?;
        let mut i = 0;
        for deployment in &self.config.deployments {
            for pkg in &deployment.packages {
                // Canonicalize the config package path for comparison
                let canonical_pkg_path = Path::new(&pkg.path).canonicalize().map_err(|e| {
                    CliError::IO(
                        format!(
                            "Failed to canonicalize package directory {}",
                            package_dir.display(),
                        ),
                        e,
                    )
                })?;
                if canonical_pkg_path == canonical_package_dir {
                    let built_package = self.build_package(
                        canonical_pkg_path.as_path(),
                        &included_args.included_artifacts,
                        move_options,
                        doc_options,
                    )?;
                    let package_address = built_package.modules().map(|m| m.address()).next().unwrap();
                    let existing_package_address = if self.is_package_address_overridden(pkg.address_name.as_str()) {
                        assert!(self.named_addresses.get(pkg.address_name.as_str()).unwrap() == package_address);
                        Some(*package_address)
                    } else {
                        None
                    };
                    let deployment = BuiltDeployment {
                        package_address: existing_package_address,
                        publisher: self
                            .config
                            .publishers
                            .get(&deployment.publisher)
                            .expect(&format!(
                                "Publisher address not found: {}",
                                deployment.publisher
                            ))
                            .clone(),
                        seed: deployment.seed.clone(),
                        pack: built_package,
                    };
                    return Ok((i, deployment));
                };
                i += 1;
            }
        }

        Err(CliError::UnexpectedError(format!(
            "No deployment found for package directory: {}",
            package_dir.display()
        )))
    }
}
