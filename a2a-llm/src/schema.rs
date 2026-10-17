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
//! OpenAI's API takes the schema as it is, and some models behind it then fill
//! every property it lists, optional or not. An optional `enum` then gets one
//! of its members, because nothing else fits: an argument nobody asked for.
//! [`optional_as_nullable`] gives each optional property `null` as a way to
//! say "none", and [`drop_added_nulls`] takes that `null` back out of the call,
//! so the tool receives what a model that omitted the property would send.
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

/// `schema` with every optional property also accepting `null`.
///
/// A property is optional when its object does not list it in `required`.
/// One that already accepts `null` is left as it is. The rewrite descends
/// through `properties` and `items`, which is where [`drop_added_nulls`]
/// looks, and nowhere else: a `$ref` or an `anyOf` branch keeps its schema.
pub fn optional_as_nullable(schema: &Value) -> Value {
    let Value::Object(object) = schema else {
        return schema.clone();
    };
    let mut out = object.clone();
    if let Some(Value::Object(properties)) = object.get("properties") {
        let required = required_of(object);
        let properties = properties
            .iter()
            .map(|(name, property)| {
                let property = optional_as_nullable(property);
                let property = if required.contains(&name.as_str()) || accepts_null(&property) {
                    property
                } else {
                    with_null(property)
                };
                (name.clone(), property)
            })
            .collect();
        out.insert("properties".into(), Value::Object(properties));
    }
    if let Some(items) = object.get("items") {
        out.insert("items".into(), optional_as_nullable(items));
    }
    Value::Object(out)
}

/// `arguments` without the `null`s [`optional_as_nullable`] let in.
///
/// `schema` is the tool's own schema, before the rewrite. A `null` is removed
/// where that schema made its property optional and did not accept `null`,
/// so the call reads as one that left the property out. Arguments that are
/// not a JSON object, or that hold no such `null`, come back unchanged.
pub fn drop_added_nulls(schema: &Value, arguments: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<Value>(arguments) else {
        return arguments.to_string();
    };
    if strip(schema, &mut value) {
        value.to_string()
    } else {
        arguments.to_string()
    }
}

/// Remove the added `null`s from `value` in place, and say whether any were.
fn strip(schema: &Value, value: &mut Value) -> bool {
    let Value::Object(object) = schema else {
        return false;
    };
    let mut changed = false;
    match value {
        Value::Object(fields) => {
            let Some(Value::Object(properties)) = object.get("properties") else {
                return false;
            };
            let required = required_of(object);
            fields.retain(|name, field| {
                let added = field.is_null()
                    && !required.contains(&name.as_str())
                    && properties.get(name).is_some_and(|p| !accepts_null(p));
                changed |= added;
                !added
            });
            for (name, field) in fields.iter_mut() {
                if let Some(property) = properties.get(name) {
                    changed |= strip(property, field);
                }
            }
        }
        Value::Array(elements) => {
            if let Some(items) = object.get("items") {
                for element in elements {
                    changed |= strip(items, element);
                }
            }
        }
        _ => {}
    }
    changed
}

fn required_of(object: &Map<String, Value>) -> Vec<&str> {
    object
        .get("required")
        .and_then(Value::as_array)
        .map(|names| names.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// Whether `schema` admits `null` as it stands.
fn accepts_null(schema: &Value) -> bool {
    let Value::Object(object) = schema else {
        // `true` admits anything, and `false` nothing worth a rewrite.
        return true;
    };
    if object.get("nullable") == Some(&Value::Bool(true)) {
        return true;
    }
    if let Some(members) = object.get("enum").and_then(Value::as_array) {
        return members.contains(&Value::Null);
    }
    if let Some(branches) = ["anyOf", "oneOf"]
        .iter()
        .find_map(|k| object.get(*k).and_then(Value::as_array))
    {
        return branches.iter().any(accepts_null);
    }
    match object.get("type") {
        Some(Value::String(name)) => name == "null",
        Some(Value::Array(names)) => names.iter().any(|n| n == "null"),
        // No `type`: `{}` admits anything, and a `$ref`, `allOf` or `const`
        // is taken not to.
        _ => !["$ref", "allOf", "const"]
            .iter()
            .any(|k| object.contains_key(*k)),
    }
}

/// `schema`, also admitting `null`.
fn with_null(schema: Value) -> Value {
    let Value::Object(mut object) = schema else {
        return schema;
    };
    let typed = matches!(object.get("type"), Some(Value::String(_) | Value::Array(_)));
    if !typed || object.contains_key("const") {
        // A combinator, a `$ref` or a `const` has no `type` of its own to
        // extend, so `null` goes beside it.
        return serde_json::json!({ "anyOf": [Value::Object(object), { "type": "null" }] });
    }
    match object.get_mut("type") {
        Some(Value::String(name)) => {
            let name = std::mem::take(name);
            object.insert(
                "type".into(),
                Value::Array(vec![name.into(), "null".into()]),
            );
        }
        Some(Value::Array(names)) => names.push("null".into()),
        _ => {}
    }
    if let Some(Value::Array(members)) = object.get_mut("enum") {
        members.push(Value::Null);
    }
    Value::Object(object)
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

    /// The case that made this: strata's `time_grain` is an optional enum,
    /// and a model that fills every property picks a grain the question
    /// never asked for.
    #[test]
    fn an_optional_enum_takes_null_and_a_required_one_does_not() {
        let schema = json!({
            "type": "object",
            "properties": {
                "metrics": { "type": "array", "items": { "type": "string", "enum": ["avg"] } },
                "time_grain": { "type": "string", "enum": ["day", "week"] },
                "time_range": { "type": "string" },
                "limit": { "type": ["integer", "null"] }
            },
            "required": ["metrics"]
        });
        let sent = optional_as_nullable(&schema);
        let properties = &sent["properties"];
        assert_eq!(properties["metrics"], schema["properties"]["metrics"]);
        assert_eq!(properties["time_grain"]["type"], json!(["string", "null"]));
        assert_eq!(
            properties["time_grain"]["enum"],
            json!(["day", "week", null])
        );
        assert_eq!(properties["time_range"]["type"], json!(["string", "null"]));
        assert_eq!(
            properties["limit"], schema["properties"]["limit"],
            "already nullable"
        );
        assert_eq!(sent["required"], schema["required"]);
    }

    /// What the tool receives is what a model that omitted the property
    /// would have sent. A `null` the tool's own schema admits is the model's
    /// to send, and stays.
    #[test]
    fn the_added_null_is_taken_back_out_and_an_admitted_one_stays() {
        let schema = json!({
            "type": "object",
            "properties": {
                "metrics": { "type": "array" },
                "time_grain": { "type": "string", "enum": ["day", "week"] },
                "limit": { "type": ["integer", "null"] },
                "order_by": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "key": { "type": "string" },
                            "direction": { "type": "string" }
                        },
                        "required": ["key"]
                    }
                }
            },
            "required": ["metrics"]
        });
        let called = r#"{"metrics":["avg"],"time_grain":null,"limit":null,"order_by":[{"key":"avg","direction":null}]}"#;
        let received: Value = serde_json::from_str(&drop_added_nulls(&schema, called)).unwrap();
        assert_eq!(
            received,
            json!({ "metrics": ["avg"], "limit": null, "order_by": [{ "key": "avg" }] })
        );
        // Nothing to take out: the text the model wrote goes through as it is.
        let plain = r#"{"metrics": ["avg"]}"#;
        assert_eq!(drop_added_nulls(&schema, plain), plain);
        assert_eq!(drop_added_nulls(&schema, "not json"), "not json");
    }

    /// A property whose schema is a `$ref` or a combinator gets `null` as a
    /// branch beside it, since it has no `type` of its own to extend.
    #[test]
    fn a_ref_or_a_combinator_gets_null_beside_it() {
        let schema = json!({
            "type": "object",
            "properties": {
                "unit": { "$ref": "#/$defs/Unit" },
                "either": { "anyOf": [{ "type": "string" }, { "type": "integer" }] },
                "maybe": { "anyOf": [{ "type": "string" }, { "type": "null" }] },
                "anything": {}
            }
        });
        let sent = optional_as_nullable(&schema);
        let properties = &sent["properties"];
        assert_eq!(
            properties["unit"],
            json!({ "anyOf": [{ "$ref": "#/$defs/Unit" }, { "type": "null" }] })
        );
        assert_eq!(
            properties["either"],
            json!({ "anyOf": [schema["properties"]["either"], { "type": "null" }] })
        );
        assert_eq!(properties["maybe"], schema["properties"]["maybe"]);
        assert_eq!(properties["anything"], json!({}));
    }
}
