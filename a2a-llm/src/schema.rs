//! What each provider takes of a tool's JSON Schema.
//!
//! [`ToolDefinition::parameters`](crate::ToolDefinition::parameters) is JSON
//! Schema, because that is what every tool source produces — MCP servers,
//! `schemars`, hand-written definitions. OpenAI's Chat Completions API takes it
//! as it is. Gemini's function declarations do not: `parameters` there is an
//! OpenAPI 3.0 `Schema` object, a fixed set of fields, and the API answers a
//! keyword outside that set with the same 400 it gives an unknown
//! `thinkingConfig` field — "Unknown name … Cannot find field" — rather than
//! ignoring it. So a tool that lists on Gemini is one whose source happened to
//! avoid `$schema`, `additionalProperties`, `const`, `$ref` and a `format` the
//! API does not spell, and a tool that does not is a 400 naming a field the
//! tool's author never wrote. [`for_gemini`] is the one place that knows the
//! difference, so a tool source does not have to know which model is on the
//! other end.
//!
//! The rewrite is by translation where a translation exists and by dropping
//! where none does: `type: ["string", "null"]` is `nullable: true`, a
//! `const` string is a one-member `enum`, a local `$ref` is inlined, and a
//! keyword with no counterpart is removed and named in
//! [`Sanitized::dropped`] so the caller can log it. A dropped keyword loosens
//! the schema — the model may then produce an argument the tool's validator
//! refuses — which is why the drops are reported rather than silent, and why
//! the translations exist for the cases where fidelity is free.

use serde_json::{Map, Value};

/// A schema rewritten for one provider, and what the rewrite cost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sanitized {
    /// The schema the provider will accept.
    pub schema: Value,
    /// JSON-pointer paths of the keywords that were removed because the
    /// provider has no spelling for them, in the order they were met. Empty
    /// when the schema went through unchanged (translations do not count: a
    /// keyword rewritten into the provider's spelling was kept, not lost).
    pub dropped: Vec<String>,
}

/// The fields of Gemini's `Schema` object, per the `generateContent` reference
/// for v1beta. Everything else is refused by the API, not ignored.
const GEMINI_KEYWORDS: &[&str] = &[
    "type",
    "format",
    "title",
    "description",
    "nullable",
    "enum",
    "maxItems",
    "minItems",
    "properties",
    "required",
    "minProperties",
    "maxProperties",
    "minLength",
    "maxLength",
    "pattern",
    "example",
    "anyOf",
    "propertyOrdering",
    "default",
    "items",
    "minimum",
    "maximum",
];

/// The `format` values Gemini documents for each type. A format outside this
/// list is refused, so `uri`, `email`, `uuid` and `schemars`' `uint32` are
/// dropped rather than sent.
fn gemini_format_allowed(type_name: Option<&str>, format: &str) -> bool {
    match type_name {
        Some("number") => matches!(format, "float" | "double"),
        Some("integer") => matches!(format, "int32" | "int64"),
        Some("string") => matches!(format, "enum" | "date-time"),
        _ => false,
    }
}

/// Rewrite a JSON Schema into what Gemini's function declarations take.
///
/// The input is what a tool source produced; the output is what goes on the
/// wire as `parameters`. A schema Gemini already accepts comes back equal to
/// its input with nothing in `dropped`.
pub fn for_gemini(schema: &Value) -> Sanitized {
    let mut rewriter = GeminiRewriter {
        root: schema,
        dropped: Vec::new(),
        refs_in_flight: Vec::new(),
    };
    let schema = rewriter.rewrite(schema, "");
    Sanitized {
        schema,
        dropped: rewriter.dropped,
    }
}

struct GeminiRewriter<'a> {
    /// The document `$ref`s are resolved against.
    root: &'a Value,
    dropped: Vec<String>,
    /// The `$ref` targets currently being inlined, so a recursive definition
    /// is cut rather than followed forever.
    refs_in_flight: Vec<String>,
}

impl GeminiRewriter<'_> {
    fn lose(&mut self, path: &str, keyword: &str) {
        self.dropped.push(format!("{path}/{keyword}"));
    }

    fn rewrite(&mut self, schema: &Value, path: &str) -> Value {
        let object = match schema {
            Value::Object(object) => object.clone(),
            // A boolean schema (`true`: anything; `false`: nothing) has no
            // OpenAPI form. Anything is the closest, and `false` as a
            // property schema is a tool nobody can call anyway.
            _ => return Value::Object(Map::new()),
        };

        let object = self.inline_ref(object, path);
        let mut out = Map::new();

        // `type` first: the other rules read it.
        let mut nullable = object
            .get("nullable")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mut type_name: Option<String> = None;
        let mut extra_types: Vec<String> = Vec::new();
        match object.get("type") {
            Some(Value::String(name)) => type_name = Some(name.clone()),
            Some(Value::Array(names)) => {
                for name in names.iter().filter_map(Value::as_str) {
                    if name == "null" {
                        nullable = true;
                    } else if type_name.is_none() {
                        type_name = Some(name.to_string());
                    } else {
                        extra_types.push(name.to_string());
                    }
                }
            }
            _ => {}
        }

        for (keyword, value) in &object {
            match keyword.as_str() {
                "type" | "nullable" => {}
                "properties" => {
                    let Value::Object(properties) = value else {
                        self.lose(path, keyword);
                        continue;
                    };
                    let rewritten: Map<String, Value> = properties
                        .iter()
                        .map(|(name, schema)| {
                            let child = format!("{path}/properties/{name}");
                            (name.clone(), self.rewrite(schema, &child))
                        })
                        .collect();
                    out.insert(keyword.clone(), Value::Object(rewritten));
                }
                "items" => {
                    if value.is_array() {
                        // Draft-4 tuple validation; Gemini's `items` is one
                        // schema.
                        self.lose(path, keyword);
                        continue;
                    }
                    let child = format!("{path}/items");
                    out.insert(keyword.clone(), self.rewrite(value, &child));
                }
                "anyOf" | "oneOf" => {
                    let Value::Array(members) = value else {
                        self.lose(path, keyword);
                        continue;
                    };
                    let mut kept = Vec::new();
                    for (index, member) in members.iter().enumerate() {
                        if member.get("type").and_then(Value::as_str) == Some("null")
                            && member.as_object().is_some_and(|m| m.len() == 1)
                        {
                            nullable = true;
                            continue;
                        }
                        let child = format!("{path}/{keyword}/{index}");
                        kept.push(self.rewrite(member, &child));
                    }
                    match kept.len() {
                        0 => {}
                        // `Option<T>` as schemars spells it: the one member
                        // left is the schema, and its keys fill in beside the
                        // parent's own.
                        1 => {
                            if let Value::Object(member) = kept.remove(0) {
                                for (k, v) in member {
                                    if k == "nullable" {
                                        nullable |= v.as_bool().unwrap_or(false);
                                    } else if k == "type" && type_name.is_none() {
                                        type_name = v.as_str().map(str::to_string);
                                    } else {
                                        out.entry(k).or_insert(v);
                                    }
                                }
                            }
                        }
                        _ => {
                            out.insert("anyOf".to_string(), Value::Array(kept));
                        }
                    }
                }
                "allOf" => {
                    // One member is just indirection; more is an intersection
                    // OpenAPI 3.0 cannot express.
                    match value.as_array().map(Vec::as_slice) {
                        Some([only]) => {
                            let child = format!("{path}/allOf/0");
                            if let Value::Object(member) = self.rewrite(only, &child) {
                                for (k, v) in member {
                                    if k == "type" && type_name.is_none() {
                                        type_name = v.as_str().map(str::to_string);
                                    } else {
                                        out.entry(k).or_insert(v);
                                    }
                                }
                            }
                        }
                        _ => self.lose(path, keyword),
                    }
                }
                "const" => match value {
                    Value::String(_) => {
                        out.insert("enum".to_string(), Value::Array(vec![value.clone()]));
                    }
                    _ => self.lose(path, keyword),
                },
                "enum" => {
                    // Gemini's `enum` members are strings whatever the type
                    // (`{type: INTEGER, enum: ["101", "201"]}` is its own
                    // example), so a number is spelled out rather than lost.
                    let Value::Array(members) = value else {
                        self.lose(path, keyword);
                        continue;
                    };
                    let members = members
                        .iter()
                        .map(|m| match m {
                            Value::String(_) => m.clone(),
                            Value::Null => Value::String("null".to_string()),
                            other => Value::String(other.to_string()),
                        })
                        .collect();
                    // A `const` seen earlier already put one here; the
                    // explicit list wins.
                    out.insert(keyword.clone(), Value::Array(members));
                }
                "format" => {
                    let allowed = value
                        .as_str()
                        .is_some_and(|f| gemini_format_allowed(type_name.as_deref(), f));
                    if allowed {
                        out.insert(keyword.clone(), value.clone());
                    } else {
                        self.lose(path, keyword);
                    }
                }
                other if GEMINI_KEYWORDS.contains(&other) => {
                    out.insert(keyword.clone(), value.clone());
                }
                _ => self.lose(path, keyword),
            }
        }

        if extra_types.is_empty() {
            if let Some(name) = type_name {
                out.insert("type".to_string(), Value::String(name));
            }
        } else if let Some(first) = type_name {
            // `type: ["string", "integer"]` is a union OpenAPI spells as
            // `anyOf`; the keywords beside it stay on the parent, where
            // Gemini reads them for whichever member matched.
            let members = std::iter::once(first)
                .chain(extra_types)
                .map(|name| serde_json::json!({ "type": name }))
                .collect();
            out.insert("anyOf".to_string(), Value::Array(members));
        }
        if nullable {
            out.insert("nullable".to_string(), Value::Bool(true));
        }

        Value::Object(out)
    }

    /// Replace a local `$ref` with what it points at, keeping the sibling
    /// keywords (draft 2019-09 allows them; a `description` beside a `$ref` is
    /// the common one). A `$ref` that cannot be resolved — remote, malformed,
    /// or already being inlined above this point — is dropped.
    fn inline_ref(&mut self, mut object: Map<String, Value>, path: &str) -> Map<String, Value> {
        let Some(reference) = object.remove("$ref") else {
            return object;
        };
        let target = reference
            .as_str()
            .filter(|r| !self.refs_in_flight.iter().any(|seen| seen == r))
            .and_then(|r| resolve_local_ref(self.root, r).map(|v| (r.to_string(), v)));
        let Some((reference, target)) = target else {
            self.lose(path, "$ref");
            return object;
        };
        self.refs_in_flight.push(reference);
        let inlined = self.rewrite(target, path);
        self.refs_in_flight.pop();

        let mut merged = match inlined {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        // Siblings override the definition; they were written for this use
        // of it. Both halves have been rewritten by the time they meet: the
        // definition just now, the siblings by the caller after this returns.
        for (k, v) in object {
            merged.insert(k, v);
        }
        merged
    }
}

/// Follow a `#/…` pointer into `root`. Only local references are resolved;
/// a tool's schema has nowhere else to point.
fn resolve_local_ref<'a>(root: &'a Value, reference: &str) -> Option<&'a Value> {
    root.pointer(reference.strip_prefix('#')?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The shape `schemars` gives a struct through rmcp's `#[tool]`: `$schema`
    /// at the top, `additionalProperties: false`, an integer with a `uint32`
    /// format, an `Option<String>` as a type array. None of those are Gemini
    /// fields, and each was a 400 until it was dropped or respelled.
    #[test]
    fn a_schemars_struct_becomes_a_gemini_schema() {
        let sanitized = for_gemini(&json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "title": "ListArgs",
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "limit": { "type": "integer", "format": "uint32", "minimum": 0 },
                "cursor": { "type": ["string", "null"], "description": "Where to resume" },
            },
            "required": ["limit"],
        }));

        assert_eq!(
            sanitized.schema,
            json!({
                "title": "ListArgs",
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "minimum": 0 },
                    "cursor": { "type": "string", "nullable": true, "description": "Where to resume" },
                },
                "required": ["limit"],
            })
        );
        assert_eq!(
            sanitized.dropped,
            vec![
                "/$schema",
                "/additionalProperties",
                "/properties/limit/format"
            ]
        );
    }

    /// A schema Gemini takes as written comes back as written, with nothing
    /// to report — the rewrite is not a normalizer.
    #[test]
    fn an_acceptable_schema_is_untouched() {
        let schema = json!({
            "type": "object",
            "description": "Look a city up",
            "properties": {
                "city": { "type": "string", "minLength": 1 },
                "units": { "type": "string", "enum": ["metric", "imperial"], "default": "metric" },
                "when": { "type": "string", "format": "date-time" },
                "days": { "type": "integer", "format": "int32", "maximum": 14 },
                "tags": { "type": "array", "items": { "type": "string" }, "maxItems": 5 },
            },
            "required": ["city"],
        });
        let sanitized = for_gemini(&schema);
        assert_eq!(sanitized.schema, schema);
        assert!(sanitized.dropped.is_empty(), "{:?}", sanitized.dropped);
    }

    /// The two keywords strata had to strip on its own side. `const` has a
    /// faithful spelling for a string and none for anything else.
    #[test]
    fn const_is_an_enum_of_one_for_a_string_and_dropped_otherwise() {
        let sanitized = for_gemini(&json!({
            "type": "object",
            "properties": {
                "kind": { "type": "string", "const": "sql" },
                "version": { "type": "integer", "const": 2 },
                "id": { "type": "string", "format": "uuid" },
            },
        }));
        assert_eq!(
            sanitized.schema["properties"],
            json!({
                "kind": { "type": "string", "enum": ["sql"] },
                "version": { "type": "integer" },
                "id": { "type": "string" },
            })
        );
        assert_eq!(
            sanitized.dropped,
            vec!["/properties/id/format", "/properties/version/const"]
        );
    }

    /// Non-string enum members are spelled out rather than dropped, which is
    /// what Gemini's own reference does with an integer enum.
    #[test]
    fn enum_members_are_strings() {
        let sanitized = for_gemini(&json!({ "type": "integer", "enum": [101, 201] }));
        assert_eq!(
            sanitized.schema,
            json!({ "type": "integer", "enum": ["101", "201"] })
        );
        assert!(sanitized.dropped.is_empty());
    }

    /// `$defs` plus `$ref` is how `schemars` shares a nested struct. The
    /// definition is inlined where it is used, a `description` beside the
    /// `$ref` survives, and `$defs` itself — a Gemini unknown — goes.
    #[test]
    fn a_local_ref_is_inlined_with_its_siblings() {
        let sanitized = for_gemini(&json!({
            "type": "object",
            "properties": {
                "target": { "$ref": "#/$defs/Target", "description": "Where to write" },
            },
            "$defs": {
                "Target": {
                    "type": "object",
                    "description": "A path",
                    "properties": { "path": { "type": "string" } },
                    "additionalProperties": false,
                },
            },
        }));
        assert_eq!(
            sanitized.schema,
            json!({
                "type": "object",
                "properties": {
                    "target": {
                        "type": "object",
                        "description": "Where to write",
                        "properties": { "path": { "type": "string" } },
                    },
                },
            })
        );
        assert_eq!(
            sanitized.dropped,
            vec!["/$defs", "/properties/target/additionalProperties"]
        );
    }

    /// A recursive definition cannot be inlined to a fixed depth that means
    /// anything; the reference that would loop is cut, and the rest kept.
    #[test]
    fn a_recursive_ref_is_cut_not_followed() {
        let sanitized = for_gemini(&json!({
            "$ref": "#/definitions/Node",
            "definitions": {
                "Node": {
                    "type": "object",
                    "properties": {
                        "value": { "type": "string" },
                        "next": { "$ref": "#/definitions/Node" },
                    },
                },
            },
        }));
        assert_eq!(
            sanitized.schema,
            json!({
                "type": "object",
                "properties": {
                    "value": { "type": "string" },
                    "next": {},
                },
            })
        );
        assert_eq!(
            sanitized.dropped,
            vec!["/properties/next/$ref", "/definitions"]
        );
    }

    /// `schemars` 1.x spells `Option<T>` for a referenced `T` as
    /// `anyOf: [{$ref}, {type: null}]`. That is one nullable schema, not a
    /// union of two.
    #[test]
    fn an_any_of_with_null_is_nullable() {
        let sanitized = for_gemini(&json!({
            "type": "object",
            "properties": {
                "target": {
                    "anyOf": [{ "$ref": "#/$defs/Target" }, { "type": "null" }],
                    "default": null,
                },
                "either": {
                    "oneOf": [{ "type": "string" }, { "type": "integer" }],
                },
            },
            "$defs": { "Target": { "type": "object", "properties": { "path": { "type": "string" } } } },
        }));
        assert_eq!(
            sanitized.schema["properties"],
            json!({
                "target": {
                    "type": "object",
                    "properties": { "path": { "type": "string" } },
                    "default": null,
                    "nullable": true,
                },
                "either": {
                    "anyOf": [{ "type": "string" }, { "type": "integer" }],
                },
            })
        );
        assert_eq!(sanitized.dropped, vec!["/$defs"]);
    }

    /// A type union with no `null` in it is the same union spelled as
    /// OpenAPI spells one.
    #[test]
    fn a_type_array_without_null_is_an_any_of() {
        let sanitized = for_gemini(&json!({ "type": ["string", "integer", "null"] }));
        assert_eq!(
            sanitized.schema,
            json!({ "anyOf": [{ "type": "string" }, { "type": "integer" }], "nullable": true })
        );
    }

    /// Tuple `items` and `allOf` of more than one have no OpenAPI 3.0 form;
    /// a single `allOf` is indirection and is flattened.
    #[test]
    fn what_has_no_spelling_is_dropped_and_named() {
        let sanitized = for_gemini(&json!({
            "type": "object",
            "properties": {
                "pair": { "type": "array", "items": [{ "type": "string" }, { "type": "integer" }] },
                "both": { "allOf": [{ "type": "string" }, { "minLength": 1 }] },
                "one": { "allOf": [{ "type": "string", "minLength": 1 }] },
                "free": true,
            },
            "patternProperties": { "^x-": {} },
        }));
        assert_eq!(
            sanitized.schema["properties"],
            json!({
                "pair": { "type": "array" },
                "both": {},
                "one": { "type": "string", "minLength": 1 },
                "free": {},
            })
        );
        assert_eq!(
            sanitized.dropped,
            vec![
                "/patternProperties",
                "/properties/both/allOf",
                "/properties/pair/items",
            ]
        );
    }
}
