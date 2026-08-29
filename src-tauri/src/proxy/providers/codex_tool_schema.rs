use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::{protocol_compatibility::ToolSchemaDialect, proxy::error::ProxyError};

pub(crate) fn compile_tool_schemas(
    body: &mut Value,
    dialect: ToolSchemaDialect,
) -> Result<(), ProxyError> {
    if dialect == ToolSchemaDialect::OpenAi {
        return Ok(());
    }
    visit_tool_values(body, "$", dialect)
}

fn visit_tool_values(
    value: &mut Value,
    path: &str,
    dialect: ToolSchemaDialect,
) -> Result<(), ProxyError> {
    match value {
        Value::Array(items) => {
            for (index, item) in items.iter_mut().enumerate() {
                visit_tool_values(item, &format!("{path}[{index}]"), dialect)?;
            }
        }
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("function") {
                if object.contains_key("parameters") {
                    let tool_name = object
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("<unnamed>")
                        .to_string();
                    let schema = object
                        .remove("parameters")
                        .expect("parameters was checked above");
                    let compiled =
                        compile_schema(schema, &tool_name, &format!("{path}.parameters"), dialect)?;
                    object.insert("parameters".to_string(), compiled.schema);
                    if compiled.relaxed {
                        object.insert("strict".to_string(), Value::Bool(false));
                    }
                } else if let Some(function) =
                    object.get_mut("function").and_then(Value::as_object_mut)
                {
                    if function.contains_key("parameters") {
                        let tool_name = function
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("<unnamed>")
                            .to_string();
                        let schema = function
                            .remove("parameters")
                            .expect("parameters was checked above");
                        let compiled = compile_schema(
                            schema,
                            &tool_name,
                            &format!("{path}.function.parameters"),
                            dialect,
                        )?;
                        function.insert("parameters".to_string(), compiled.schema);
                        if compiled.relaxed {
                            function.insert("strict".to_string(), Value::Bool(false));
                        }
                    }
                }
            }

            for (key, child) in object.iter_mut() {
                visit_tool_values(child, &format!("{path}.{key}"), dialect)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn compile_schema(
    schema: Value,
    tool_name: &str,
    path: &str,
    dialect: ToolSchemaDialect,
) -> Result<CompiledToolSchema, ProxyError> {
    match dialect {
        ToolSchemaDialect::OpenAi => Ok(CompiledToolSchema {
            schema,
            relaxed: false,
        }),
        ToolSchemaDialect::MoonshotMfjs => MfjsCompiler::new(schema, tool_name).compile(path),
    }
}

struct CompiledToolSchema {
    schema: Value,
    relaxed: bool,
}

struct MfjsCompiler<'a> {
    original: Value,
    tool_name: &'a str,
    aliases: BTreeMap<String, String>,
    definitions: BTreeMap<String, Value>,
    relaxed: bool,
}

impl<'a> MfjsCompiler<'a> {
    fn new(original: Value, tool_name: &'a str) -> Self {
        Self {
            original,
            tool_name,
            aliases: BTreeMap::new(),
            definitions: BTreeMap::new(),
            relaxed: false,
        }
    }

    fn compile(mut self, path: &str) -> Result<CompiledToolSchema, ProxyError> {
        if let Some(root) = self.original.as_object_mut() {
            let has_explicit_root_shape = ["type", "$ref", "anyOf", "oneOf", "enum", "const"]
                .iter()
                .any(|key| root.contains_key(*key));
            if !has_explicit_root_shape {
                root.insert("type".to_string(), Value::String("object".to_string()));
            }
        }
        let mut original = self.original.clone();
        let project_root_union = if let Some(root) = original.as_object_mut() {
            if root.contains_key("oneOf") && root.contains_key("anyOf") {
                return Err(self.error(
                    path,
                    "combined root oneOf and anyOf cannot be represented safely in MFJS",
                ));
            }
            // Chat conversion normalizes every function parameter root to
            // `type: object`. A real Codex dynamic tool may still carry a
            // `null` union branch, which becomes an impossible
            // `object ∩ null` branch if we compile the outer type first.
            // Root projection restores the object constraint after retaining
            // every callable object branch, so remove only this redundant
            // outer type before compiling the union.
            let has_root_union = root.contains_key("oneOf") || root.contains_key("anyOf");
            if has_root_union && root.get("type").and_then(Value::as_str) == Some("object") {
                root.remove("type");
            }
            if let Some(one_of) = root.remove("oneOf") {
                root.insert("anyOf".to_string(), one_of);
                true
            } else {
                root.contains_key("anyOf")
            }
        } else {
            false
        };
        let mut stack = vec!["#".to_string()];
        let mut compiled = self.compile_node(original, path, &mut stack)?;

        while let Some((alias, reference)) = self
            .aliases
            .iter()
            .find(|(alias, _)| !self.definitions.contains_key(*alias))
            .map(|(alias, reference)| (alias.clone(), reference.clone()))
        {
            let target = self.resolve_local_ref(&reference, path)?;
            let mut definition_stack = vec![reference.clone()];
            let definition = self.compile_node(
                target,
                &format!("{path}.$defs.{alias}"),
                &mut definition_stack,
            )?;
            self.definitions.insert(alias, definition);
        }

        if !self.definitions.is_empty() {
            let root = compiled.as_object_mut().ok_or_else(|| {
                self.error(path, "recursive schema root must compile to an object")
            })?;
            root.insert(
                "$defs".to_string(),
                Value::Object(std::mem::take(&mut self.definitions).into_iter().collect()),
            );
        }
        let projected_root_union =
            project_root_union && compiled.get("type").and_then(Value::as_str) != Some("object");
        if projected_root_union {
            compiled = self.project_root_object_union(compiled, path)?;
            self.relaxed = true;
        }
        if compiled.get("type").and_then(Value::as_str) != Some("object") {
            return Err(self.error(
                path,
                "tool parameters must compile to an object root in MFJS",
            ));
        }
        Ok(CompiledToolSchema {
            schema: compiled,
            relaxed: self.relaxed,
        })
    }

    fn project_root_object_union(&self, compiled: Value, path: &str) -> Result<Value, ProxyError> {
        project_root_object_union_schema(compiled)
            .map_err(|message| self.error(path, message))?
            .ok_or_else(|| self.error(path, "root union must contain oneOf or anyOf"))
    }

    fn compile_node(
        &mut self,
        node: Value,
        path: &str,
        stack: &mut Vec<String>,
    ) -> Result<Value, ProxyError> {
        match node {
            Value::Bool(true) => Ok(any_schema()),
            Value::Bool(false) => Err(self.error(
                path,
                "the always-false JSON Schema cannot be represented safely in MFJS",
            )),
            Value::Object(mut object) => {
                // MFJS explicitly rejects `title` and `$comment`. They are JSON
                // Schema annotations, so removing them does not change the set of
                // accepted tool arguments.
                object.remove("title");
                object.remove("$comment");
                // `format` and the remaining OpenAI/JSON-Schema metadata below
                // are not part of MFJS. They do not define the object shape the
                // client needs to deserialize, so drop them and mark the tool
                // non-strict instead of rejecting an otherwise usable Codex
                // dynamic tool such as automation_update.
                for annotation in [
                    "format",
                    "$schema",
                    "$anchor",
                    "$dynamicAnchor",
                    "$vocabulary",
                    "deprecated",
                    "examples",
                    "readOnly",
                    "writeOnly",
                ] {
                    if object.remove(annotation).is_some() {
                        self.relaxed = true;
                    }
                }
                if let Some(reference) = object.remove("$ref") {
                    let reference = reference
                        .as_str()
                        .ok_or_else(|| self.error(path, "$ref must be a string"))?;
                    let siblings = object;
                    if stack.iter().any(|entry| entry == reference) {
                        self.validate_recursive_siblings(reference, &siblings, path)?;
                        let alias = self.alias_for(reference);
                        return Ok(json!({"$ref": format!("#/$defs/{alias}")}));
                    }
                    let target = self.resolve_local_ref(reference, path)?;
                    let merged = self.merge_schema_values(target, Value::Object(siblings), path)?;
                    stack.push(reference.to_string());
                    let compiled = self.compile_node(merged, path, stack);
                    stack.pop();
                    return compiled;
                }
                self.compile_object(object, path, stack)
            }
            _ => Err(self.error(path, "JSON Schema must be an object or boolean")),
        }
    }

    fn compile_object(
        &mut self,
        mut source: Map<String, Value>,
        path: &str,
        stack: &mut Vec<String>,
    ) -> Result<Value, ProxyError> {
        source.remove("$defs");
        source.remove("definitions");

        for keyword in [
            "allOf",
            "not",
            "if",
            "then",
            "else",
            "dependentSchemas",
            "dependentRequired",
            "contains",
            "patternProperties",
            "propertyNames",
            "prefixItems",
            "unevaluatedProperties",
            "unevaluatedItems",
            "pattern",
            "exclusiveMinimum",
            "exclusiveMaximum",
            "multipleOf",
            "minProperties",
            "maxProperties",
            "uniqueItems",
            "minContains",
            "maxContains",
            "$dynamicAnchor",
            "$dynamicRef",
            "contentEncoding",
            "contentMediaType",
            "contentSchema",
        ] {
            if source.contains_key(keyword) {
                return Err(self.error(
                    &format!("{path}.{keyword}"),
                    &format!("{keyword} constraints cannot be represented safely in MFJS"),
                ));
            }
        }

        if source.contains_key("oneOf") && source.contains_key("anyOf") {
            return Err(self.error(
                path,
                "combined oneOf and anyOf cannot be represented safely in MFJS",
            ));
        }
        if let Some(one_of) = source.remove("oneOf") {
            let branches = one_of
                .as_array()
                .ok_or_else(|| self.error(&format!("{path}.oneOf"), "oneOf must be an array"))?;
            if branches.is_empty() || !one_of_branches_are_pairwise_disjoint(branches) {
                return Err(self.error(
                    &format!("{path}.oneOf"),
                    "oneOf can only be represented as MFJS anyOf when every branch is provably disjoint",
                ));
            }
            source.insert("anyOf".to_string(), one_of);
        }
        if source.contains_key("const") && source.contains_key("enum") {
            return Err(self.error(
                path,
                "combined const and enum cannot be represented safely in MFJS",
            ));
        }
        if let Some(constant) = source.remove("const") {
            source.insert("enum".to_string(), Value::Array(vec![constant]));
        }

        let description = take_string_annotation(&mut source, "description", path, self)?;
        let default = source.remove("default");
        let mut constraints = Map::new();
        for key in [
            "type",
            "enum",
            "properties",
            "required",
            "additionalProperties",
            "items",
            "minLength",
            "maxLength",
            "minimum",
            "maximum",
            "minItems",
            "maxItems",
        ] {
            if let Some(value) = source.remove(key) {
                constraints.insert(key.to_string(), value);
            }
        }

        let any_of = source.remove("anyOf");
        if let Some((keyword, _)) = source.iter().next() {
            return Err(self.error(
                &format!("{path}.{keyword}"),
                &format!("{keyword} is not supported by Moonshot MFJS"),
            ));
        }
        let mut result = if let Some(any_of) = any_of {
            let children = any_of
                .as_array()
                .ok_or_else(|| self.error(&format!("{path}.anyOf"), "anyOf must be an array"))?;
            if children.is_empty() {
                return Err(self.error(
                    &format!("{path}.anyOf"),
                    "empty anyOf cannot be represented safely",
                ));
            }
            let mut compiled = Vec::with_capacity(children.len());
            for (index, child) in children.iter().cloned().enumerate() {
                let merged = self.merge_schema_values(
                    child,
                    Value::Object(constraints.clone()),
                    &format!("{path}.anyOf[{index}]"),
                )?;
                compiled.push(self.compile_node(
                    merged,
                    &format!("{path}.anyOf[{index}]"),
                    stack,
                )?);
            }
            json!({"anyOf": compiled})
        } else {
            self.compile_constraints(constraints, path, stack)?
        };

        if let Some(object) = result.as_object_mut() {
            if let Some(description) = description {
                object.insert("description".to_string(), description);
            }
            if let Some(default) = default {
                object.insert("default".to_string(), default);
            }
        }
        Ok(result)
    }

    fn compile_constraints(
        &mut self,
        mut source: Map<String, Value>,
        path: &str,
        stack: &mut Vec<String>,
    ) -> Result<Value, ProxyError> {
        if let Some(type_value) = source.remove("type") {
            if let Some(types) = type_value.as_array() {
                if types.is_empty() {
                    return Err(self.error(&format!("{path}.type"), "type array cannot be empty"));
                }
                let mut variants = Vec::with_capacity(types.len());
                for (index, item_type) in types.iter().enumerate() {
                    let item_type = item_type.as_str().ok_or_else(|| {
                        self.error(
                            &format!("{path}.type[{index}]"),
                            "type array entries must be strings",
                        )
                    })?;
                    let mut variant = source.clone();
                    variant.insert("type".to_string(), Value::String(item_type.to_string()));
                    variants.push(self.compile_constraints(
                        variant,
                        &format!("{path}.type[{index}]"),
                        stack,
                    )?);
                }
                return Ok(json!({"anyOf": variants}));
            }
            let item_type = type_value.as_str().ok_or_else(|| {
                self.error(&format!("{path}.type"), "type must be a string or array")
            })?;
            validate_mfjs_type(item_type).map_err(|message| self.error(path, message))?;
            retain_constraints_for_type(&mut source, item_type);
            source.insert("type".to_string(), Value::String(item_type.to_string()));
        }

        if let Some(enum_value) = source.get("enum").cloned() {
            let inferred = infer_enum_type(&enum_value)
                .map_err(|message| self.error(&format!("{path}.enum"), message))?;
            match source.get("type").and_then(Value::as_str) {
                Some("number") if inferred == "integer" => {}
                Some(existing) if existing != inferred => {
                    return Err(self.error(path, "enum values do not match the declared type"));
                }
                None => {
                    source.insert("type".to_string(), Value::String(inferred.to_string()));
                }
                _ => {}
            }
        }

        if let Some(properties) = source.remove("properties") {
            let properties = properties.as_object().ok_or_else(|| {
                self.error(
                    &format!("{path}.properties"),
                    "properties must be an object",
                )
            })?;
            let mut compiled = Map::new();
            for (name, schema) in properties {
                compiled.insert(
                    name.clone(),
                    self.compile_node(schema.clone(), &format!("{path}.properties.{name}"), stack)?,
                );
            }
            if let Some(required) = source.get("required").and_then(Value::as_array) {
                for (index, name) in required.iter().enumerate() {
                    let name = name.as_str().ok_or_else(|| {
                        self.error(
                            &format!("{path}.required[{index}]"),
                            "required entries must be strings",
                        )
                    })?;
                    if !compiled.contains_key(name) {
                        let schema = self
                            .compile_missing_required_property_schema(&source, path, name, stack)?;
                        compiled.insert(name.to_string(), schema);
                    }
                }
            }
            source.insert("properties".to_string(), Value::Object(compiled));
        } else if let Some(required) = source.get("required").and_then(Value::as_array) {
            let mut compiled = Map::new();
            for (index, name) in required.iter().enumerate() {
                let name = name.as_str().ok_or_else(|| {
                    self.error(
                        &format!("{path}.required[{index}]"),
                        "required entries must be strings",
                    )
                })?;
                let schema =
                    self.compile_missing_required_property_schema(&source, path, name, stack)?;
                compiled.insert(name.to_string(), schema);
            }
            source.insert("properties".to_string(), Value::Object(compiled));
        }

        if let Some(required) = source.get("required") {
            if !required.is_array() {
                return Err(self.error(&format!("{path}.required"), "required must be an array"));
            }
        }

        if let Some(items) = source.remove("items") {
            if items.is_array() {
                return Err(self.error(
                    &format!("{path}.items"),
                    "tuple items cannot be represented safely in Moonshot MFJS",
                ));
            }
            let compiled = self.compile_node(items, &format!("{path}.items"), stack)?;
            source.insert("items".to_string(), compiled);
        }

        if let Some(additional) = source.remove("additionalProperties") {
            let compiled = match additional {
                Value::Bool(value) => Value::Bool(value),
                schema => {
                    self.compile_node(schema, &format!("{path}.additionalProperties"), stack)?
                }
            };
            source.insert("additionalProperties".to_string(), compiled);
        }

        validate_bounds(&source, path).map_err(|message| self.error(path, &message))?;

        if source.get("type").is_none() && source.get("enum").is_none() {
            return Ok(expand_missing_type(source));
        }
        Ok(Value::Object(source))
    }

    fn compile_missing_required_property_schema(
        &mut self,
        source: &Map<String, Value>,
        path: &str,
        name: &str,
        stack: &mut Vec<String>,
    ) -> Result<Value, ProxyError> {
        match source.get("additionalProperties") {
            None | Some(Value::Bool(true)) => Ok(any_schema()),
            Some(Value::Bool(false)) => Err(self.error(
                &format!("{path}.required"),
                &format!("required property `{name}` is forbidden by additionalProperties=false"),
            )),
            Some(schema @ Value::Object(_)) => self.compile_node(
                schema.clone(),
                &format!("{path}.additionalProperties"),
                stack,
            ),
            Some(_) => Err(self.error(
                &format!("{path}.additionalProperties"),
                "additionalProperties must be a boolean or object schema",
            )),
        }
    }

    fn resolve_local_ref(&self, reference: &str, path: &str) -> Result<Value, ProxyError> {
        if !reference.starts_with('#') {
            return Err(self.error(path, "remote $ref values are not supported by MFJS"));
        }
        if reference == "#" {
            return Ok(self.original.clone());
        }
        let pointer = reference.strip_prefix('#').unwrap_or_default();
        self.original
            .pointer(pointer)
            .cloned()
            .ok_or_else(|| self.error(path, &format!("unresolved local $ref `{reference}`")))
    }

    fn validate_recursive_siblings(
        &self,
        reference: &str,
        siblings: &Map<String, Value>,
        path: &str,
    ) -> Result<(), ProxyError> {
        let meaningful = siblings
            .iter()
            .filter(|(key, _)| !matches!(key.as_str(), "description" | "default" | "$defs"))
            .collect::<Vec<_>>();
        if meaningful.is_empty() {
            return Ok(());
        }
        if meaningful.len() == 1 && meaningful[0].0 == "type" {
            let target = self.resolve_local_ref(reference, path)?;
            if target.get("type") == Some(meaningful[0].1) {
                return Ok(());
            }
        }
        Err(self.error(
            path,
            "recursive $ref sibling constraints cannot be represented safely in MFJS",
        ))
    }

    fn alias_for(&mut self, reference: &str) -> String {
        if let Some((alias, _)) = self
            .aliases
            .iter()
            .find(|(_, value)| value.as_str() == reference)
        {
            return alias.clone();
        }
        let stem = reference
            .rsplit('/')
            .next()
            .unwrap_or("root")
            .replace('~', "_")
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || character == '_' {
                    character
                } else {
                    '_'
                }
            })
            .collect::<String>();
        let digest = format!("{:x}", Sha256::digest(reference.as_bytes()));
        let alias = format!(
            "{}_{}",
            if stem.is_empty() { "root" } else { &stem },
            &digest[..8]
        );
        self.aliases.insert(alias.clone(), reference.to_string());
        alias
    }

    fn merge_schema_values(
        &self,
        target: Value,
        sibling: Value,
        path: &str,
    ) -> Result<Value, ProxyError> {
        let mut target = target
            .as_object()
            .cloned()
            .ok_or_else(|| self.error(path, "$ref target must be an object schema"))?;
        let sibling = sibling
            .as_object()
            .cloned()
            .ok_or_else(|| self.error(path, "$ref sibling schema must be an object"))?;
        for (key, value) in sibling {
            if matches!(key.as_str(), "$defs" | "definitions") {
                continue;
            }
            match target.get_mut(&key) {
                None => {
                    target.insert(key, value);
                }
                Some(existing) if *existing == value => {}
                Some(existing) if key == "description" || key == "default" => {
                    *existing = value;
                }
                Some(existing) if key == "title" => {
                    *existing = value;
                }
                Some(existing) if key == "type" => {
                    *existing = intersect_types(existing, &value).ok_or_else(|| {
                        self.error(
                            &format!("{path}.type"),
                            "intersecting type constraints have no common value",
                        )
                    })?;
                }
                Some(existing) if key == "enum" => {
                    let left = existing
                        .as_array()
                        .ok_or_else(|| self.error(path, "enum must be an array"))?;
                    let right = value
                        .as_array()
                        .ok_or_else(|| self.error(path, "enum must be an array"))?;
                    let intersection = left
                        .iter()
                        .filter(|item| right.contains(item))
                        .cloned()
                        .collect::<Vec<_>>();
                    if intersection.is_empty() {
                        return Err(self.error(
                            &format!("{path}.enum"),
                            "intersecting enum constraints have no common value",
                        ));
                    }
                    *existing = Value::Array(intersection);
                }
                Some(existing) if matches!(key.as_str(), "minLength" | "minItems" | "minimum") => {
                    *existing =
                        stricter_numeric_bound(existing, &value, true).ok_or_else(|| {
                            self.error(
                                &format!("{path}.{key}"),
                                "numeric bounds must be finite JSON numbers",
                            )
                        })?;
                }
                Some(existing) if matches!(key.as_str(), "maxLength" | "maxItems" | "maximum") => {
                    *existing =
                        stricter_numeric_bound(existing, &value, false).ok_or_else(|| {
                            self.error(
                                &format!("{path}.{key}"),
                                "numeric bounds must be finite JSON numbers",
                            )
                        })?;
                }
                Some(existing) if key == "required" => {
                    let mut required = existing
                        .as_array()
                        .cloned()
                        .ok_or_else(|| self.error(path, "required must be an array"))?;
                    let incoming = value
                        .as_array()
                        .ok_or_else(|| self.error(path, "required must be an array"))?;
                    for item in incoming {
                        if !required.contains(item) {
                            required.push(item.clone());
                        }
                    }
                    *existing = Value::Array(required);
                }
                Some(existing) if key == "properties" => {
                    let mut properties = existing
                        .as_object()
                        .cloned()
                        .ok_or_else(|| self.error(path, "properties must be an object"))?;
                    let incoming = value
                        .as_object()
                        .ok_or_else(|| self.error(path, "properties must be an object"))?;
                    for (name, schema) in incoming {
                        match properties.get(name) {
                            None => {
                                properties.insert(name.clone(), schema.clone());
                            }
                            Some(current) if current == schema => {}
                            Some(current) => {
                                properties.insert(
                                    name.clone(),
                                    self.merge_schema_values(
                                        current.clone(),
                                        schema.clone(),
                                        &format!("{path}.properties.{name}"),
                                    )?,
                                );
                            }
                        }
                    }
                    *existing = Value::Object(properties);
                }
                Some(existing) if key == "items" => {
                    *existing = self.merge_schema_values(
                        existing.clone(),
                        value,
                        &format!("{path}.items"),
                    )?;
                }
                Some(existing) if key == "additionalProperties" => {
                    *existing =
                        intersect_additional_properties(existing.clone(), value, path, self)?;
                }
                Some(_) => {
                    return Err(self.error(
                        &format!("{path}.{key}"),
                        "intersecting JSON Schema constraints cannot be represented safely in MFJS",
                    ));
                }
            }
        }
        Ok(Value::Object(target))
    }

    fn error(&self, path: &str, message: &str) -> ProxyError {
        ProxyError::TransformError(format!(
            "tool `{}` schema at `{path}` is incompatible with Moonshot MFJS: {message}",
            self.tool_name
        ))
    }
}

fn take_string_annotation<'a>(
    source: &mut Map<String, Value>,
    keyword: &str,
    path: &str,
    compiler: &MfjsCompiler<'a>,
) -> Result<Option<Value>, ProxyError> {
    let Some(value) = source.remove(keyword) else {
        return Ok(None);
    };
    if value.is_string() {
        Ok(Some(value))
    } else {
        Err(compiler.error(
            &format!("{path}.{keyword}"),
            &format!("{keyword} must be a string"),
        ))
    }
}

/// Project a root `oneOf`/`anyOf` that contains callable object branches into
/// one plain object schema. Strict third-party Responses validators reject the
/// union root used by Codex dynamic tools, while the model still needs every
/// branch's fields. The projection therefore widens property schemas, keeps
/// only requirements shared by every object branch, and preserves definitions.
/// Callers must disable strict validation when this returns `Some`.
pub(crate) fn project_root_object_union_schema(
    schema: Value,
) -> Result<Option<Value>, &'static str> {
    let mut root = schema
        .as_object()
        .cloned()
        .ok_or("root union must be an object schema")?;
    if root.contains_key("oneOf") && root.contains_key("anyOf") {
        return Err("combined root oneOf and anyOf cannot be projected safely");
    }
    let variants = if let Some(value) = root.remove("oneOf") {
        value
    } else if let Some(value) = root.remove("anyOf") {
        value
    } else {
        return Ok(None);
    };
    let variants = variants
        .as_array()
        .ok_or("root oneOf or anyOf must be an array")?;
    let mut objects = Vec::new();
    for variant in variants {
        collect_object_union_variants(variant, &mut objects);
    }
    if objects.is_empty() {
        return Err("root union contains no object branch usable as tool parameters");
    }

    let mut property_names = BTreeSet::new();
    for object in &objects {
        if let Some(properties) = object.get("properties").and_then(Value::as_object) {
            property_names.extend(properties.keys().cloned());
        }
    }

    let mut properties = Map::new();
    for property_name in property_names {
        let mut alternatives = Vec::new();
        for object in &objects {
            if let Some(schema) = object
                .get("properties")
                .and_then(Value::as_object)
                .and_then(|properties| properties.get(&property_name))
            {
                push_unique_schema(&mut alternatives, schema.clone());
                continue;
            }
            match object.get("additionalProperties") {
                Some(Value::Bool(false)) => {}
                Some(Value::Object(schema)) => {
                    push_unique_schema(&mut alternatives, Value::Object(schema.clone()));
                }
                _ => {
                    alternatives.clear();
                    alternatives.push(any_schema());
                    break;
                }
            }
        }
        if !alternatives.is_empty() {
            properties.insert(property_name, schema_union(alternatives));
        }
    }

    let mut required = objects
        .first()
        .map(required_property_names)
        .unwrap_or_default();
    for object in objects.iter().skip(1) {
        let current = required_property_names(object);
        required.retain(|name| current.contains(name));
    }

    let mut result = Map::new();
    result.insert("type".to_string(), Value::String("object".to_string()));
    result.insert("properties".to_string(), Value::Object(properties));
    if !required.is_empty() {
        result.insert(
            "required".to_string(),
            Value::Array(required.into_iter().map(Value::String).collect()),
        );
    }
    if let Some(additional) = union_additional_properties(&objects) {
        result.insert("additionalProperties".to_string(), additional);
    }
    for annotation in ["description", "default", "$defs", "definitions"] {
        if let Some(value) = root.remove(annotation) {
            result.insert(annotation.to_string(), value);
        }
    }
    Ok(Some(Value::Object(result)))
}

fn collect_object_union_variants(schema: &Value, objects: &mut Vec<Map<String, Value>>) {
    let Some(object) = schema.as_object() else {
        return;
    };
    if object.get("type").and_then(Value::as_str) == Some("object") {
        objects.push(object.clone());
        return;
    }
    for union_key in ["oneOf", "anyOf"] {
        if let Some(variants) = object.get(union_key).and_then(Value::as_array) {
            for variant in variants {
                collect_object_union_variants(variant, objects);
            }
        }
    }
}

fn push_unique_schema(schemas: &mut Vec<Value>, schema: Value) {
    if !schemas.contains(&schema) {
        schemas.push(schema);
    }
}

fn schema_union(mut schemas: Vec<Value>) -> Value {
    if schemas.len() == 1 {
        schemas.pop().expect("one schema was checked above")
    } else {
        json!({"anyOf": schemas})
    }
}

fn required_property_names(object: &Map<String, Value>) -> BTreeSet<String> {
    object
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn union_additional_properties(objects: &[Map<String, Value>]) -> Option<Value> {
    let mut schemas = Vec::new();
    for object in objects {
        match object.get("additionalProperties") {
            None | Some(Value::Bool(true)) => return None,
            Some(Value::Bool(false)) => {}
            Some(schema @ Value::Object(_)) => {
                push_unique_schema(&mut schemas, schema.clone());
            }
            Some(_) => return None,
        }
    }
    if schemas.is_empty() {
        Some(Value::Bool(false))
    } else {
        Some(schema_union(schemas))
    }
}

fn one_of_branches_are_pairwise_disjoint(branches: &[Value]) -> bool {
    branches.iter().enumerate().all(|(left_index, left)| {
        branches
            .iter()
            .skip(left_index + 1)
            .all(|right| schemas_are_provably_disjoint(left, right))
    })
}

fn schemas_are_provably_disjoint(left: &Value, right: &Value) -> bool {
    let Some(left) = left.as_object() else {
        return false;
    };
    let Some(right) = right.as_object() else {
        return false;
    };

    if let (Some(left_values), Some(right_values)) = (literal_values(left), literal_values(right)) {
        return left_values
            .iter()
            .all(|value| !right_values.contains(value));
    }

    if let (Some(left_types), Some(right_types)) = (
        declared_types(left.get("type")),
        declared_types(right.get("type")),
    ) {
        return left_types.iter().all(|left_type| {
            right_types
                .iter()
                .all(|right_type| types_are_disjoint(left_type, right_type))
        });
    }

    false
}

fn literal_values(schema: &Map<String, Value>) -> Option<Vec<Value>> {
    if let Some(value) = schema.get("const") {
        return Some(vec![value.clone()]);
    }
    schema.get("enum").and_then(Value::as_array).cloned()
}

fn declared_types(value: Option<&Value>) -> Option<Vec<&str>> {
    match value? {
        Value::String(value) => Some(vec![value.as_str()]),
        Value::Array(values) => values.iter().map(Value::as_str).collect(),
        _ => None,
    }
}

fn types_are_disjoint(left: &str, right: &str) -> bool {
    left != right && !matches!((left, right), ("number", "integer") | ("integer", "number"))
}

fn intersect_types(left: &Value, right: &Value) -> Option<Value> {
    let left_types = declared_types(Some(left))?;
    let right_types = declared_types(Some(right))?;
    let mut intersection = Vec::new();
    for left_type in left_types {
        for right_type in &right_types {
            let common = if left_type == *right_type {
                Some(left_type)
            } else if matches!(
                (left_type, *right_type),
                ("number", "integer") | ("integer", "number")
            ) {
                Some("integer")
            } else {
                None
            };
            if let Some(common) = common {
                if !intersection.contains(&common) {
                    intersection.push(common);
                }
            }
        }
    }
    match intersection.as_slice() {
        [] => None,
        [only] => Some(Value::String((*only).to_string())),
        many => Some(Value::Array(
            many.iter()
                .map(|value| Value::String((*value).to_string()))
                .collect(),
        )),
    }
}

fn stricter_numeric_bound(left: &Value, right: &Value, lower: bool) -> Option<Value> {
    let left_number = left.as_f64()?;
    let right_number = right.as_f64()?;
    if (lower && right_number > left_number) || (!lower && right_number < left_number) {
        Some(right.clone())
    } else {
        Some(left.clone())
    }
}

fn intersect_additional_properties<'a>(
    left: Value,
    right: Value,
    path: &str,
    compiler: &MfjsCompiler<'a>,
) -> Result<Value, ProxyError> {
    match (left, right) {
        (Value::Bool(false), _) | (_, Value::Bool(false)) => Ok(Value::Bool(false)),
        (Value::Bool(true), value) | (value, Value::Bool(true)) => Ok(value),
        (left @ Value::Object(_), right @ Value::Object(_)) => {
            compiler.merge_schema_values(left, right, &format!("{path}.additionalProperties"))
        }
        _ => Err(compiler.error(
            &format!("{path}.additionalProperties"),
            "additionalProperties must be a boolean or object schema",
        )),
    }
}

fn retain_constraints_for_type(source: &mut Map<String, Value>, item_type: &str) {
    let allowed = match item_type {
        "object" => &["properties", "required", "additionalProperties"][..],
        "array" => &["items", "minItems", "maxItems"][..],
        "string" => &["minLength", "maxLength"][..],
        "number" | "integer" => &["minimum", "maximum"][..],
        _ => &[][..],
    };
    source.retain(|key, _| key == "enum" || allowed.contains(&key.as_str()));
}

fn validate_bounds(source: &Map<String, Value>, path: &str) -> Result<(), String> {
    for keyword in ["minLength", "maxLength", "minItems", "maxItems"] {
        if let Some(value) = source.get(keyword) {
            if value.as_u64().is_none() {
                return Err(format!("{path}.{keyword} must be a non-negative integer"));
            }
        }
    }
    for keyword in ["minimum", "maximum"] {
        if source
            .get(keyword)
            .is_some_and(|value| value.as_f64().is_none())
        {
            return Err(format!("{path}.{keyword} must be a JSON number"));
        }
    }
    for (minimum, maximum) in [
        ("minLength", "maxLength"),
        ("minItems", "maxItems"),
        ("minimum", "maximum"),
    ] {
        if let (Some(lower), Some(upper)) = (
            source.get(minimum).and_then(Value::as_f64),
            source.get(maximum).and_then(Value::as_f64),
        ) {
            if lower > upper {
                return Err(format!("{path}.{minimum} must not exceed {maximum}"));
            }
        }
    }
    Ok(())
}

fn validate_mfjs_type(item_type: &str) -> Result<(), &'static str> {
    if matches!(
        item_type,
        "null" | "boolean" | "object" | "array" | "number" | "integer" | "string"
    ) {
        Ok(())
    } else {
        Err("unsupported JSON Schema type")
    }
}

fn infer_enum_type(value: &Value) -> Result<&'static str, &'static str> {
    let values = value.as_array().ok_or("enum must be an array")?;
    if values.is_empty() {
        return Err("empty enum cannot be represented safely");
    }
    let mut kinds = BTreeSet::new();
    for value in values {
        let kind = match value {
            Value::String(_) => "string",
            Value::Number(number) if number.is_i64() || number.is_u64() => "integer",
            Value::Number(_) => "number",
            _ => return Err("MFJS enum values must be strings or numbers"),
        };
        kinds.insert(kind);
    }
    if kinds.len() == 1 {
        return Ok(kinds.into_iter().next().expect("one enum kind"));
    }
    if kinds
        .iter()
        .all(|kind| matches!(*kind, "integer" | "number"))
    {
        return Ok("number");
    }
    Err("mixed-type enum cannot be represented safely in MFJS")
}

fn any_schema() -> Value {
    json!({
        "anyOf": [
            {"type": "null"},
            {"type": "boolean"},
            {"type": "object"},
            {"type": "array"},
            {"type": "number"},
            {"type": "string"}
        ]
    })
}

fn expand_missing_type(mut constraints: Map<String, Value>) -> Value {
    let object_properties = constraints.remove("properties");
    let object_required = constraints.remove("required");
    let object_additional = constraints.remove("additionalProperties");
    let array_items = constraints.remove("items");
    let array_min_items = constraints.remove("minItems");
    let array_max_items = constraints.remove("maxItems");
    let number_minimum = constraints.remove("minimum");
    let number_maximum = constraints.remove("maximum");
    let string_min_length = constraints.remove("minLength");
    let string_max_length = constraints.remove("maxLength");

    let mut variants = vec![
        json!({"type": "null"}),
        json!({"type": "boolean"}),
        json!({"type": "object"}),
        json!({"type": "array"}),
        json!({"type": "number"}),
        json!({"type": "string"}),
    ];
    if let Some(object) = variants[2].as_object_mut() {
        if let Some(value) = object_properties {
            object.insert("properties".to_string(), value);
        }
        if let Some(value) = object_required {
            object.insert("required".to_string(), value);
        }
        if let Some(value) = object_additional {
            object.insert("additionalProperties".to_string(), value);
        }
    }
    if let Some(array) = variants[3].as_object_mut() {
        if let Some(value) = array_items {
            array.insert("items".to_string(), value);
        }
        if let Some(value) = array_min_items {
            array.insert("minItems".to_string(), value);
        }
        if let Some(value) = array_max_items {
            array.insert("maxItems".to_string(), value);
        }
    }
    if let Some(number) = variants[4].as_object_mut() {
        if let Some(value) = number_minimum {
            number.insert("minimum".to_string(), value);
        }
        if let Some(value) = number_maximum {
            number.insert("maximum".to_string(), value);
        }
    }
    if let Some(string) = variants[5].as_object_mut() {
        if let Some(value) = string_min_length {
            string.insert("minLength".to_string(), value);
        }
        if let Some(value) = string_max_length {
            string.insert("maxLength".to_string(), value);
        }
    }
    json!({"anyOf": variants})
}
