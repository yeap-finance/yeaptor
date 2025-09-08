# Yeaptor

Yeaptor provides a small CLI and a Move package that make deterministic, admin‑gated deployments of Move code to Aptos resource ("package") accounts straightforward and repeatable.

Core focus in this repository:
- crates/yeaptor — a CLI that turns a declarative `yeaptor.toml` plan into ready‑to‑run Aptos entry‑function JSON payloads.
- packages/resource-account-code-deployment — a Move module that deploys/upgrades packages to package accounts derived from (publisher, seed), with admin control and optional freeze.

## Highlights
- Deterministic package account addresses from (publisher, seed) with domain‑separated seeds
- One‑shot publish/upgrade via entry functions
- Admin‑gated operations using aptos_extensions::manageable
- Optional freeze to lock code and revoke management
- Simple, declarative CLI workflow that outputs JSON payloads for `aptos move run`

## What’s inside
- CLI: `crates/yeaptor` (binary name: `yeaptor`)
- Move package: `packages/resource-account-code-deployment` (module `ra_code_deployment::ra_code_deployment`)
- Also available (not the focus here):
  - `packages/object-code-deterministic-deployment`
  - `packages/proxy-account`

## How it works
- You describe deployments in `yeaptor.toml` using logical publisher aliases, a seed, and a list of packages to deploy under the same package account.
- The CLI computes the package account address for each deployment via the module’s `create_package_address(publisher, seed)` function (domain‑separated).
- During build, the CLI injects named addresses so each package’s `address_name` resolves to that resource account address.
- For each package, the CLI emits a JSON payload calling `<yeaptor_address>::ra_code_deployment::deploy(seed, metadata, modules)`.
- You submit the JSON payloads in order with Aptos CLI.

## Prerequisites
- Rust toolchain (stable)
- Aptos CLI installed and configured (profiles, network, etc.)

## Install the CLI
- From repo root: `cargo install --path crates/yeaptor`
- Or run without installing: `cargo run -p yeaptor -- <args>`

## Quick start
1) Bootstrap or reference the `ra_code_deployment` module on‑chain
   - Either publish `packages/resource-account-code-deployment` once to an address you control, or reuse an existing deployment. Note that the CLI needs this address as `yeaptor_address` to invoke the `deploy`/`batch_deploy` entry functions.
2) Create `yeaptor.toml`
   - Start from `yeaptor.toml.example` and set:
     - `yeaptor_address` to the on‑chain address hosting `ra_code_deployment`.
     - `[publishers]` mapping for your logical names to addresses.
     - `[[deployments]]` with a UTF‑8 `seed` and package list.
3) Generate publish payloads
   - `yeaptor deployment build --config ./yeaptor.toml --out-dir ./deployments`
4) Submit payloads in order
   - `aptos move run --profile <profile> --json-file ./deployments/<n>-<pkg>.package.json`

## Configuration (yeaptor.toml)
Keys:
- format_version: Schema version. Use `1`.
- yeaptor_address: On‑chain address where `ra_code_deployment` is published.
- [publishers]: Map of alias -> on‑chain address. Referenced by `deployments.publisher`.
- [named-addresses] (optional): Extra Move named addresses shared across packages.
- [[deployments]]: Ordered list. Each defines one resource account derived from `(publisher + seed)` and its ordered packages.
  - publisher: Alias from `[publishers]` or a literal address string.
  - seed: UTF‑8 text used to deterministically derive the resource account.
  - packages: Array of `{ address_name, path }` where:
    - address_name: Named address used by the package (will resolve to the derived resource account).
    - path: Filesystem path to the Move package (containing `Move.toml`).

Example:

```
format_version = 1
yeaptor_address = "0x<address-hosting-ra_code_deployment>"

[publishers]
yeap-multisig = "0x10"

[named-addresses]
# std = "0x1"

[[deployments]]
publisher = "yeap-multisig"
seed = "core-v1"            # UTF-8 only
packages = [
  { address_name = "ra_code_deployment", path = "packages/resource-account-code-deployment" },
  { address_name = "proxy_account",      path = "packages/proxy-account" },
]
```

Generated outputs
- Files are written to `--out-dir` in deployment order: `<index>-<package>.package.json`.
- If `--with-event` is provided to `deployment build`, event files are written under `--out-dir/events/<package>.event.json`.
- An `addresses.toml` with resolved named addresses is also written to `--out-dir`.
- Each publish file calls:
```json
{
  "function_id": "0x<yeaptor_address>::ra_code_deployment::deploy",
  "type_args": [],
  "args": [
    { "type": "hex", "value": "0x<seed-bytes>" },
    { "type": "hex", "value": "0x<metadata-bcs>" },
    { "type": "hex", "value": ["0x<module-1>", "0x<module-2>", ...] }
  ]
}
```

Notes
- Order matters: deployments and the packages within them are processed sequentially.
- `seed` must be UTF‑8 text (not hex). The module applies domain separation internally for deterministic address derivation.
- `address_name` must match the named address used in the package’s `Move.toml`.
- `yeaptor_address` must be the on‑chain address hosting the `ra_code_deployment` module.

## CLI features

### 1) Deployment build
Turn a declarative `yeaptor.toml` plan into ready‑to‑run Aptos entry‑function JSON payloads.

- Command
  - `yeaptor deployment build --config ./yeaptor.toml --out-dir ./deployments`
  - Build one package only: add `--package-dir <path/to/package>`
  - Include event definitions alongside payloads: add `--with-event` (writes to `<out-dir>/events/`)
- Outputs
  - `<out-dir>/<index>-<package>.package.json` per package
  - `<out-dir>/events/<package>.event.json` (when `--with-event`)
  - `<out-dir>/addresses.toml` resolved named addresses
- Submit payloads
  - `aptos move run --profile <profile> --json-file <out-dir>/<index>-<package>.package.json`

### 2) Event generation
Generate per‑package event definition JSON files from compiled Move packages.

- Command
  - All from config: `yeaptor event generate --config ./yeaptor.toml --out-dir ./events`
  - Single package: `yeaptor event generate --config ./yeaptor.toml --out-dir ./events --package-dir ./packages/<pkg>`
- Output
  - `./events/<package>.event.json` files (array of event definitions with fields/types)

### 3) Processor config generation (no‑code indexer)
Generate, don’t run, a processor configuration YAML that can be used by a no‑code/indexer pipeline.

- Command
  - `yeaptor processor generate --starting-version <u64> \
      --events-dir ./events \
      --db_schema ./db_schema.csv \
      --event_mapping ./event_mappings.csv \
      --output-file ./processor_config.yaml`
- Inputs
  - Event definitions directory (`--events-dir`), typically from “Event generation” or `deployment build --with-event`
  - Database schema CSV (`--db_schema`)
  - Event‑to‑table mapping CSV (`--event_mapping`)
- Output
  - `processor_config.yaml` with:
    - `custom_config.db_schema`: tables/columns
    - `custom_config.events`: event→table/column mapping
    - `custom_config.transaction_metadata` and `custom_config.event_metadata`
- Notes
  - This doesn’t run an indexer; it only produces the config for downstream use.

## Move module: ra_code_deployment::ra_code_deployment
Deterministic deployment and upgrade of Move packages to package accounts using a publisher‑provided seed (with domain‑separated seeds).

Concepts
- Deterministic address (domain‑separated): `create_package_address(publisher, seed)`.
- Admin model: Uses `aptos_extensions::manageable` to gate publish/upgrade to admins.
- Capability storage: `PublishPackageCap` (stored under the package account) holds the `SignerCapability` to sign upgrades.

Public API
- `#[view] create_package_address(publisher: address, seed: vector<u8>): address`
  - Deterministically derives the package account address from `(publisher, seed)` with domain separation.
- `entry fun create_package_account(publisher: &signer, seed: vector<u8>)`
  - Creates the package account; stores `PublishPackageCap` and initializes manageable admin with `publisher` as admin.
- `entry fun deploy(publisher: &signer, seed: vector<u8>, metadata_serialized: vector<u8>, code: vector<vector<u8>>)` acquires `PublishPackageCap`
  - Ensures the package account exists, then publishes the package to that account (upgrade if already published).
- `entry fun publish(admin: &signer, metadata_serialized: vector<u8>, code: vector<vector<u8>>, resource_address: address)` acquires `PublishPackageCap`
  - Requires admin; publishes/upgrades using the stored capability.
- `entry fun freeze_package_account(admin: &signer, resource_address: address)` acquires `PublishPackageCap`
  - Admin‑gated. Revokes management and removes the stored capability to prevent further publishes/upgrades.

Storage under the package account
- `PublishPackageCap { cap: SignerCapability }`
- Manageable admin resource (via `aptos_extensions::manageable`)

## Typical use cases
- Protocol‑owned modules at a stable, pre‑known address per `(publisher, seed)`
- Managed admin‑gated upgrades during rollout; freeze when finalized
- Multi‑package deployments to a shared resource account

## Project status
Targets the Aptos mainnet framework specified by each `Move.toml`. Review, testing, and audits are recommended before production use.

## Contributing
Issues and PRs are welcome. Please include clear repro steps and tests where possible.

## License
Apache‑2.0. See `LICENSE`.
