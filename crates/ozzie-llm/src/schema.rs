//! Tool schema normalization for LLM providers.
//!
//! schemars generates JSON Schema Draft 7 with fields that many LLM APIs
//! don't expect (`$schema`, `title`, `description`, `format`, validation
//! constraints). This module provides shared normalization logic.

use schemars::schema::{InstanceType, RootSchema, Schema, SchemaObject, SingleOrVec};

/// How to represent nullable types after normalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NullableStyle {
    /// Drop null from the type array, add `"nullable": true` extension (Gemini/OpenAPI style).
    Extension,
    /// Drop null from the type array, no marker (OpenAI style — nullable is implicit).
    Drop,
}

/// What to strip from the schema.
#[derive(Debug, Clone)]
pub struct NormalizeOptions {
    pub nullable_style: NullableStyle,
    /// Strip `format` field (Gemini rejects it; OpenAI ignores it).
    pub strip_format: bool,
    /// Strip numeric/string validation constraints (min, max, pattern, etc.).
    pub strip_constraints: bool,
    /// Set `additionalProperties: false` on all objects (Anthropic best practice).
    pub force_additional_properties_false: bool,
    /// Add all property names to `required` (Anthropic best practice).
    pub force_all_required: bool,
}

impl NormalizeOptions {
    /// Strict normalization for Gemini (strip everything it rejects).
    pub fn gemini() -> Self {
        Self {
            nullable_style: NullableStyle::Extension,
            strip_format: true,
            strip_constraints: true,
            force_additional_properties_false: false,
            force_all_required: false,
        }
    }

    /// Light normalization for OpenAI-compatible APIs (llama.cpp, vLLM, OpenAI).
    pub fn openai() -> Self {
        Self {
            nullable_style: NullableStyle::Drop,
            strip_format: false,
            strip_constraints: false,
            force_additional_properties_false: false,
            force_all_required: false,
        }
    }

    /// Anthropic normalization (force strict schemas for better tool use).
    pub fn anthropic() -> Self {
        Self {
            nullable_style: NullableStyle::Drop,
            strip_format: false,
            strip_constraints: false,
            force_additional_properties_false: true,
            force_all_required: true,
        }
    }
}

/// Normalizes a RootSchema in place.
///
/// Strips `$schema` and `title` meta-fields, then recurses into all properties,
/// array items, and subschemas to normalize nullable types and optionally strip
/// format/constraints.
pub fn normalize_schema(root: &mut RootSchema, opts: &NormalizeOptions) {
    // Strip root-level meta ($schema, title, description)
    root.schema.metadata = None;
    root.meta_schema = None;

    normalize_obj(&mut root.schema, opts);
}

fn normalize_obj(obj: &mut SchemaObject, opts: &NormalizeOptions) {
    // Strip title from nested objects (keep description on properties — useful for LLMs)
    if let Some(ref meta) = obj.metadata
        && meta.title.is_some()
    {
        let mut m = (**meta).clone();
        m.title = None;
        if m == schemars::schema::Metadata::default() {
            obj.metadata = None;
        } else {
            obj.metadata = Some(Box::new(m));
        }
    }

    // Convert ["type", "null"] → "type" (+ optional nullable extension)
    if let Some(SingleOrVec::Vec(types)) = &obj.instance_type {
        let non_null: Vec<InstanceType> = types
            .iter()
            .filter(|t| **t != InstanceType::Null)
            .cloned()
            .collect();
        let had_null = non_null.len() < types.len();
        if had_null {
            obj.instance_type = if non_null.len() == 1 {
                Some(SingleOrVec::Single(Box::new(non_null[0])))
            } else {
                Some(SingleOrVec::Vec(non_null))
            };
            if opts.nullable_style == NullableStyle::Extension {
                obj.extensions
                    .insert("nullable".to_string(), serde_json::Value::Bool(true));
            }
        }
    }

    // Strip format
    if opts.strip_format {
        obj.format = None;
    }

    // Strip constraints
    if opts.strip_constraints {
        if let Some(ref mut num) = obj.number {
            num.minimum = None;
            num.maximum = None;
            num.exclusive_minimum = None;
            num.exclusive_maximum = None;
            num.multiple_of = None;
        }
        if let Some(ref mut s) = obj.string {
            s.min_length = None;
            s.max_length = None;
            s.pattern = None;
        }
    }

    // Recurse into properties + apply Anthropic strictness
    if let Some(ref mut object) = obj.object {
        if opts.force_all_required {
            let all_keys: Vec<String> = object.properties.keys().cloned().collect();
            for key in all_keys {
                object.required.insert(key);
            }
        }
        if opts.force_additional_properties_false && object.additional_properties.is_none() {
            object.additional_properties = Some(Box::new(Schema::Bool(false)));
        }
        for prop in object.properties.values_mut() {
            normalize_ref(prop, opts);
        }
        if let Some(ref mut additional) = object.additional_properties {
            normalize_ref(additional, opts);
        }
    }

    // Recurse into array items
    if let Some(ref mut array) = obj.array
        && let Some(ref mut items) = array.items
    {
        match items {
            SingleOrVec::Single(s) => normalize_ref(s, opts),
            SingleOrVec::Vec(v) => {
                for s in v {
                    normalize_ref(s, opts);
                }
            }
        }
    }

    // Recurse into subschemas (anyOf, oneOf, allOf)
    if let Some(ref mut sub) = obj.subschemas {
        for list in [&mut sub.any_of, &mut sub.one_of, &mut sub.all_of] {
            for s in list.iter_mut().flatten() {
                normalize_ref(s, opts);
            }
        }
    }
}

fn normalize_ref(schema: &mut Schema, opts: &NormalizeOptions) {
    if let Schema::Object(obj) = schema {
        normalize_obj(obj, opts);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemars::schema::*;

    fn make_schema_with_optional() -> RootSchema {
        RootSchema {
            meta_schema: Some("http://json-schema.org/draft-07/schema#".to_string()),
            schema: SchemaObject {
                metadata: Some(Box::new(Metadata {
                    title: Some("TestArgs".to_string()),
                    description: Some("Test tool args.".to_string()),
                    ..Default::default()
                })),
                instance_type: Some(SingleOrVec::Single(Box::new(InstanceType::Object))),
                object: Some(Box::new(ObjectValidation {
                    properties: {
                        let mut m = schemars::Map::new();
                        m.insert(
                            "name".to_string(),
                            Schema::Object(SchemaObject {
                                instance_type: Some(SingleOrVec::Vec(vec![
                                    InstanceType::String,
                                    InstanceType::Null,
                                ])),
                                ..Default::default()
                            }),
                        );
                        m.insert(
                            "count".to_string(),
                            Schema::Object(SchemaObject {
                                instance_type: Some(SingleOrVec::Single(Box::new(
                                    InstanceType::Integer,
                                ))),
                                ..Default::default()
                            }),
                        );
                        m
                    },
                    ..Default::default()
                })),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn openai_style_strips_null_and_meta() {
        let mut schema = make_schema_with_optional();
        normalize_schema(&mut schema, &NormalizeOptions::openai());

        let v = serde_json::to_value(&schema).unwrap();
        // $schema and title stripped
        assert!(v.get("$schema").is_none());
        assert!(v.get("title").is_none());
        assert!(v.get("description").is_none());
        // nullable type simplified
        assert_eq!(v["properties"]["name"]["type"], "string");
        assert!(v["properties"]["name"].get("nullable").is_none()); // no extension
        // non-nullable unchanged
        assert_eq!(v["properties"]["count"]["type"], "integer");
    }

    #[test]
    fn gemini_style_adds_nullable_extension() {
        let mut schema = make_schema_with_optional();
        normalize_schema(&mut schema, &NormalizeOptions::gemini());

        let v = serde_json::to_value(&schema).unwrap();
        assert_eq!(v["properties"]["name"]["type"], "string");
        assert_eq!(v["properties"]["name"]["nullable"], true);
        assert!(v["properties"]["count"].get("nullable").is_none());
    }

    #[test]
    fn anthropic_style_forces_strict_schema() {
        let mut schema = make_schema_with_optional();
        normalize_schema(&mut schema, &NormalizeOptions::anthropic());

        let v = serde_json::to_value(&schema).unwrap();
        // additionalProperties: false
        assert_eq!(v["additionalProperties"], false);
        // All properties are required
        let required = v["required"].as_array().unwrap();
        assert!(required.contains(&serde_json::json!("name")));
        assert!(required.contains(&serde_json::json!("count")));
        // Nullable type simplified (Drop style, no extension)
        assert_eq!(v["properties"]["name"]["type"], "string");
        assert!(v["properties"]["name"].get("nullable").is_none());
    }
}
