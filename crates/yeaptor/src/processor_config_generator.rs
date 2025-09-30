use crate::event_definition::EventDefinition;
use crate::processor_config::{
    ColumnTarget, CommonConfig, CustomConfig, EventMapping, ProcessorConfig, SpecIdentifier,
    TableSchema,
};
use anyhow::{Context, anyhow};
use aptos::common::init::Network;
use aptos_types::transaction::Version;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
const EVENT_METADATA: &str = "event_metadata";
const EVENT_METADATA_FIELDS: &[&str] = &[
    "account_address",
    "creation_number",
    EVENT_INDEX,
    EVENT_TYPE,
    "sequence_number",
];
const EVENT_INDEX: &str = "event_index";
const EVENT_TYPE: &str = "event_type";

const TRANSACTION_METADATA: &str = "transaction_metadata";
const TRANSACTION_METADATA_FIELDS: &[&str] = &["block_height", "epoch", "timestamp", "version", "hash"];
pub fn load_event_definitions_from_dir(dir: &Path) -> anyhow::Result<Vec<EventDefinition>> {
    let mut out: Vec<EventDefinition> = Vec::new();
    for entry in
        fs::read_dir(dir).with_context(|| format!("failed to read dir: {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Some(ext) = path.extension() {
            if ext != "json" {
                continue;
            }
        } else {
            continue;
        }
        let data = fs::read_to_string(&path)
            .with_context(|| format!("failed to read file: {}", path.display()))?;
        let defs: Vec<EventDefinition> = serde_json::from_str(&data)
            .with_context(|| format!("failed to parse JSON in {}", path.display()))?;
        out.extend(defs);
    }
    Ok(out)
}

pub fn generate_processor_config(
    network: Network,
    starting_version: Version,
    event_definitions: &[EventDefinition],
    // table schema
    table_schemas: &BTreeMap<String, TableSchema>,
    // event -> table mapping
    event_mapping: &BTreeMap<String, Vec<String>>,
) -> anyhow::Result<(ProcessorConfig, Vec<String>, Vec<(String, String)>)> {
    let mut mapped_table_columns = BTreeMap::new();
    let mut unmapped_events = Vec::new();

    // handle events
    let mut mapped_events = BTreeMap::new();
    for event_definition in event_definitions {
        let event_name = format!(
            "{}::{}::{}",
            &event_definition.package_name, &event_definition.module_name, &event_definition.name
        );

        let custom_mapped_fields =
            event_mapping
                .iter()
                .fold(BTreeMap::new(), |mut mapped_events, (k, v)| {
                    let stripped = k.strip_prefix(&event_name).filter(|s| !s.is_empty());
                    if let Some(custom_field) = stripped {
                        let custom_field = custom_field
                            .strip_prefix("::")
                            .ok_or(anyhow!(format!(
                                "invalid format of custom event mapping, {}",
                                k
                            )))
                            .unwrap();

                        mapped_events.insert(
                            custom_field.to_string(),
                            v.iter()
                                .filter_map(|m| {
                                    m.split_once("::").map(|(table, column)| ColumnTarget {
                                        column: column.to_string(),
                                        table: table.to_string(),
                                    })
                                })
                                .collect::<Vec<_>>(),
                        );
                    }
                    mapped_events
                });

        let mapped_tables = event_mapping.get(&event_name);
        if mapped_tables.is_none() {
            unmapped_events.push(event_name);
            continue;
        }

        let mapped_tables = mapped_tables.unwrap();
        let mut event_fields = BTreeMap::new();
        for (field_name, _field_type) in &event_definition.fields {
            let mut column_targets = vec![];
            for mapped_table in mapped_tables {
                let table_schema = table_schemas.get(mapped_table).ok_or(anyhow!(format!(
                    "Table schema for mapping {} -> {} not found",
                    &event_name, &mapped_table
                )))?;
                if table_schema.contains_key(field_name) {
                    mapped_table_columns
                        .entry(mapped_table.clone())
                        .or_insert_with(Vec::new)
                        .push(field_name.clone());
                    column_targets.push(ColumnTarget {
                        column: field_name.clone(),
                        table: mapped_table.clone(),
                    });
                } else if custom_mapped_fields.contains_key(field_name) {
                    for column_target in custom_mapped_fields.get(field_name).unwrap() {
                        let _ = table_schemas
                            .get(column_target.table.as_str())
                            .and_then(|schema| {
                                if schema.contains_key(&column_target.column) {
                                    Some(schema)
                                } else {
                                    None
                                }
                            })
                            .ok_or(anyhow!(format!(
                                "Table Column for mapping {}::{} -> {}::{} not found",
                                &event_name,
                                &field_name,
                                &column_target.table,
                                &column_target.column
                            )))?;
                        mapped_table_columns
                            .entry(column_target.table.clone())
                            .or_insert_with(Vec::new)
                            .push(column_target.column.clone());
                        column_targets.push(column_target.clone());
                    }
                }
            }
            if !column_targets.is_empty() {
                let key = format!("$.{}", field_name);
                event_fields.insert(key, column_targets);
            } else {
                unmapped_events.push(format!("{}::{}", &event_name, field_name));
            }
        }
        let mut event_metadata = BTreeMap::new();
        for key in [
            "account_address",
            "creation_number",
            EVENT_INDEX,
            EVENT_TYPE,
            "sequence_number",
        ] {
            let targets = mapped_tables
                .iter()
                .filter_map(|mapped_table| {
                    table_schemas
                        .get(mapped_table)
                        .unwrap()
                        .iter()
                        .find(|(_column_name, column_spec)| {
                            column_spec.column_type.r#type == EVENT_METADATA
                                && column_spec.column_type.column_type == key
                        })
                        .map(|item| ColumnTarget {
                            table: mapped_table.to_string(),
                            column: item.0.to_string(),
                        })
                })
                .collect::<Vec<_>>();
            targets.iter().for_each(|target| {
                mapped_table_columns
                    .entry(target.table.clone())
                    .or_insert_with(Vec::new)
                    .push(target.column.clone());
            });
            event_metadata.insert(key.to_string(), targets);
        }

        let materialized_event_name = format!(
            "{}::{}::{}",
            &event_definition.module_address, &event_definition.module_name, &event_definition.name
        );
        mapped_events.insert(
            materialized_event_name,
            EventMapping {
                constant_values: Vec::new(),
                event_fields,
                event_metadata,
            },
        );
    }

    // handle transaction metadata
    let mut transaction_metadata = BTreeMap::new();
    for key in TRANSACTION_METADATA_FIELDS {
        let targets = table_schemas
            .iter()
            .filter_map(|(table_name, schema)| {
                schema
                    .iter()
                    .find(|(_column_name, column_spec)| {
                        &column_spec.column_type.r#type == TRANSACTION_METADATA
                            && &column_spec.column_type.column_type == key
                    })
                    .map(|(column_name, _)| ColumnTarget {
                        table: table_name.clone(),
                        column: column_name.clone(),
                    })
            })
            .collect::<Vec<_>>();
        targets.iter().for_each(|target| {
            mapped_table_columns
                .entry(target.table.clone())
                .or_insert_with(Vec::new)
                .push(target.column.clone());
        });
        transaction_metadata.insert(key.to_string(), targets);
    }

    // handle event metadata
    let mut event_metadata = BTreeMap::new();
    for key in EVENT_METADATA_FIELDS {
        let targets = table_schemas
            .iter()
            .filter_map(|(table_name, schema)| {
                schema
                    .iter()
                    .find(|(_column_name, column_spec)| {
                        &column_spec.column_type.r#type == EVENT_METADATA
                            && &column_spec.column_type.column_type == key
                    })
                    .map(|(column_name, _)| ColumnTarget {
                        table: table_name.clone(),
                        column: column_name.clone(),
                    })
            })
            .collect::<Vec<_>>();
        targets.iter().for_each(|target| {
            mapped_table_columns
                .entry(target.table.clone())
                .or_insert_with(Vec::new)
                .push(target.column.clone());
        });
        event_metadata.insert(key.to_string(), targets);
    }

    // find_unmapped_table_columns(table_schemas, &mapped_table_columns)
    //     .into_iter()
    //     .for_each(|(table_name, column_name)| {
    //         eprintln!("Warning: Column '{}' in table '{}' is not mapped by any event or transaction metadata.", column_name, table_name);
    //     });

    let config = ProcessorConfig {
        spec_identifier: SpecIdentifier {
            spec_creator: "shepherd@aptoslabs.com".to_string(),
            spec_name: "remapping-processor".to_string(),
            spec_version: "0.0.10".to_string(),
        },
        common_config: CommonConfig {
            network: network.to_string(),
            starting_version,
            starting_version_override: None,
        },

        custom_config: CustomConfig {
            payload: BTreeMap::new(),
            db_schema: table_schemas.clone(),
            events: mapped_events,
            transaction_metadata,
            event_metadata,
        },
    };
    Ok((
        config,
        unmapped_events,
        find_unmapped_table_columns(table_schemas, &mapped_table_columns),
    ))
}

fn find_unmapped_table_columns(
    table_schemas: &BTreeMap<String, TableSchema>,
    mapped_table_columns: &BTreeMap<String, Vec<String>>,
) -> Vec<(String, String)> {
    table_schemas
        .iter()
        .flat_map(|(table_name, schema)| {
            schema.iter().filter_map(|(column_name, _)| {
                if !mapped_table_columns
                    .get(table_name)
                    .map_or(false, |columns| columns.contains(column_name))
                {
                    Some((table_name.clone(), column_name.clone()))
                } else {
                    None
                }
            })
        })
        .collect()
}
