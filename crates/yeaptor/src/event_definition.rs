use aptos_types::account_address::AccountAddress;
use aptos_types::vm::module_metadata::RuntimeModuleMetadataV1;
use move_binary_format::CompiledModule;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use move_binary_format::views::ModuleView;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventDefinition {
    pub package_name: String,
    pub module_address: AccountAddress,
    pub module_name: String,
    pub name: String,
    pub fields: BTreeMap<String, String>,
}

pub(crate) fn extract_event_definitions(
    module: &CompiledModule,
) -> BTreeMap<String, BTreeMap<String, String>> {
    let metadata = aptos_types::vm::module_metadata::get_metadata_from_compiled_code(module);
    if metadata.is_none() {
        return BTreeMap::new();
    }
    let metadata = metadata.unwrap();
    let events = extract_event_metadata(&metadata);

    let view = ModuleView::new(module);

    view.structs()

        .filter(|s| events.contains(s.name().as_str()))
        .map(|s| {
            let fields = s
                .fields().map(|f|
                f.map(|f| (f.name().to_string(), format!("{:?}", f.signature_token()).to_lowercase()))
                .collect::<BTreeMap<_, _>>());
            (s.name().to_string(), fields.unwrap_or_default())
        })
        .collect::<BTreeMap<_, _>>()
}

pub(crate) fn extract_event_metadata(metadata: &RuntimeModuleMetadataV1) -> HashSet<String> {
    let mut event_structs = HashSet::new();
    for (struct_, attrs) in &metadata.struct_attributes {
        for attr in attrs {
            if attr.is_event() {
                event_structs.insert(struct_.clone());
            }
        }
    }
    event_structs
}
