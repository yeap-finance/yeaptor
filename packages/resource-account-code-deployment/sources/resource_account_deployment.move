// Copyright (c) 2025 yeap-finance
// SPDX-License-Identifier: Apache-2.0

/// Module providing deterministic deployment of Move packages to resource accounts ("package accounts").
///
/// This module exposes helper functions to:
/// - Derive the package account address off-chain or on-chain without writing state (create_package_address [view])
/// - Create a deterministic package account for a publisher and seed (create_package_account)
/// - Publish or upgrade a package under that account (publish)
/// - Freeze the package account to disable future publishes/upgrades (freeze_package_account)
/// - Idempotently ensure the account exists and publish in one call (deploy)
///
/// Notes
/// - Seeds are domain-separated to avoid collisions with other modules. The effective derivation uses the
///   module address (as BCS bytes) followed by the ASCII string "::ra_code_deployment::" and then the caller-provided seed.
/// - All address derivations used here are deterministic and compatible across create_package_address and create_package_account.
module ra_code_deployment::ra_code_deployment {
    use std::bcs;
    use std::signer::address_of;
    use aptos_framework::account;
    use aptos_framework::account::{SignerCapability, create_resource_address};
    use aptos_framework::code;
    use aptos_extensions::manageable;

    /// Capability to create a signer for the resource account in order to upgrade code.
    struct PublishPackageCap has key {
        cap: SignerCapability
    }

    /// Domain-separate the input seed to avoid collisions with other modules using the same seed.
    ///
    /// Effective derivation prefix: BCS(@ra_code_deployment) || b"::ra_code_deployment::" || seed
    fun domain_separated_seed(seed: vector<u8>): vector<u8> {
        let s = bcs::to_bytes(&@ra_code_deployment);
        s.append(b"::ra_code_deployment::");
        s.append(seed);
        s
    }

    #[view]
    /// Deterministically derive the package account address for `publisher` and `seed`.
    ///
    /// - Pure/view function: does not write state.
    /// - Uses the same domain-separated seed scheme as creation/publish flows.
    public fun create_package_address(publisher: address, seed: vector<u8>): address {
        create_resource_address(&publisher, domain_separated_seed(seed))
    }

    /// Create the resource (package) account for the given `publisher` and `seed`.
    ///
    /// - Creates the resource account derived from `publisher` and `seed` using domain-separated derivation.
    /// - Stores a `PublishPackageCap` under the resource account for future publishes/upgrades.
    /// - Initializes a manageable admin resource with `publisher` as admin.
    ///
    /// Note: This function does not check for prior existence and will abort if the account
    /// already exists.
    public entry fun create_package_account(publisher: &signer, seed: vector<u8>) {
        let (resource, resource_signer_cap) = account::create_resource_account(publisher, domain_separated_seed(seed));
        move_to(&resource, PublishPackageCap { cap: resource_signer_cap });
        manageable::new(&resource, address_of(publisher));
    }

    /// Freeze a package account by revoking management and removing the publish capability.
    ///
    /// - Requires `admin` to be an admin of the manageable resource at `resource_address`.
    /// - Moves out the `PublishPackageCap` from the resource account, preventing future publishes/upgrades.
    /// - Generates a signer from that capability and calls `manageable::destroy` to remove the
    ///   manageable admin resource from the resource account.
    ///
    /// Effects:
    /// - After execution, the resource account is no longer manageable via this module and cannot
    ///   publish or upgrade packages using the removed capability.
    public entry fun freeze_package_account(admin: &signer, resource_address: address) acquires PublishPackageCap {
        manageable::assert_is_admin(admin, resource_address);
        let PublishPackageCap {cap} = move_from<PublishPackageCap>(resource_address);
        let resource_signer = account::create_signer_with_capability(&cap);
        manageable::destroy(&resource_signer);
    }

    /// Deploy a package to a deterministic package account derived from `publisher` and `seed`.
    ///
    /// Ensures the package account exists (via `create_package_account`) and then publishes the
    /// package by calling `publish`. Idempotent with respect to account creation.
    public entry fun deploy(publisher: &signer, seed: vector<u8>, metadata_serialized: vector<u8>, code: vector<vector<u8>>) acquires PublishPackageCap {
        // Use a domain-separated seed for address derivation to ensure unique, predictable addresses
        // for this module without colliding with other seed consumers.
        let resource_address = create_package_address(address_of(publisher), seed);
        if (!exists<PublishPackageCap>(resource_address)) {
            create_package_account(publisher, seed);
        };
        publish(publisher, metadata_serialized, code, resource_address);
    }

    /// Publish (or upgrade) a package to `resource_address`.
    ///
    /// - Requires `admin` to be an admin of the manageable resource at `resource_address`.
    /// - Uses the stored `PublishPackageCap` to create a signer for the resource account and publish
    ///   the package with `metadata_serialized` and `code`. Calling this again will upgrade the package.
    public entry fun publish(admin: &signer, metadata_serialized: vector<u8>, code: vector<vector<u8>>, resource_address: address) acquires PublishPackageCap {
        manageable::assert_is_admin(admin, resource_address);
        let deploy_cap = borrow_global<PublishPackageCap>(resource_address);
        let resource_signer = account::create_signer_with_capability(&deploy_cap.cap);
        code::publish_package_txn(&resource_signer, metadata_serialized, code);
    }
}
