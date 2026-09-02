use std::collections::HashMap;

use http::header::{AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde_json::{json, Value};

use crate::protocol_compatibility::{HistoryReplay, ToolSchemaDialect};
use crate::provider::{LocalProxyRequestOverrides, Provider, ProviderMeta};

use super::codex_request::{
    CodexRequestOptions, CodexRequestTransport, CodexThirdPartyRequestPolicy,
};

fn third_party_provider(api_format: &str) -> Provider {
    let mut provider = Provider::with_id(
        "third-party-provider".to_string(),
        "Third Party".to_string(),
        json!({
            "auth": {"OPENAI_API_KEY": "request-policy-secret"},
            "config": format!(r#"model = "visible-model"
model_provider = "third-party"
[model_providers.third-party]
base_url = "https://gateway.example/custom/v1"
wire_api = "{}"
"#, if api_format == "openai_chat" { "chat" } else { "responses" }),
            "apiFormat": api_format,
            "modelCatalog": {"models": [{
                "model": "visible-model",
                "upstreamModel": "upstream/model-v2",
                "apiFormat": api_format,
                "reasoning": {
                    "schemaVersion": 2,
                    "supportStatus": "confirmed_supported",
                    "controlKind": "graded",
                    "supportedEfforts": ["low", "medium", "xhigh"],
                    "defaultEffort": "medium",
                    "disableAllowed": false,
                    "upstream": {
                        "format": "string",
                        "parameter": "reasoning_effort",
                        "effortMap": {
                            "low": "low",
                            "medium": "medium",
                            "high": "xhigh",
                            "xhigh": "xhigh",
                            "max": "xhigh"
                        }
                    },
                    "outputFormat": "reasoning_content"
                }
            }]}
        }),
        None,
    );
    provider.meta = Some(ProviderMeta {
        api_format: Some(api_format.to_string()),
        custom_user_agent: Some("CCSM-Contract/2".to_string()),
        local_proxy_request_overrides: Some(LocalProxyRequestOverrides {
            headers: HashMap::from([
                ("X-Provider-Policy".to_string(), "enabled".to_string()),
                ("Authorization".to_string(), "Bearer forbidden".to_string()),
                ("Content-Type".to_string(), "text/plain".to_string()),
            ]),
            body: Some(json!({
                "metadata": {"probePolicy": "shared"},
                "stream": false
            })),
        }),
        ..ProviderMeta::default()
    });
    provider
}

fn logical_request() -> Value {
    json!({
        "model": "visible-model",
        "input": [{
            "role": "user",
            "content": [{"type": "input_text", "text": "contract probe"}]
        }],
        "reasoning": {"effort": "high", "summary": "auto"},
        "max_output_tokens": 128,
        "stream": true,
        "_privateProbeField": "must-not-leak"
    })
}

fn realistic_codex_tool_request() -> Value {
    json!({
        "model": "visible-model",
        "input": [
            {
                "type": "reasoning",
                "id": "rs_strict_replay",
                "summary": [{
                    "type": "summary_text",
                    "text": "Plan the next automation update."
                }],
                "encrypted_content": "opaque-official-replay"
            },
            {
                "role": "user",
                "content": [{"type": "input_text", "text": "update the automation"}]
            }
        ],
        "tools": [{
            "type": "function",
            "name": "codex_app__automation_update",
            "description": "Create or update an automation.",
            "parameters": {
                "type": "object",
                "properties": {
                    "destination": {
                        "$ref": "#/$defs/__schema20",
                        "type": "object",
                        "description": "Destination selected by the user."
                    },
                    "mode": {
                        "oneOf": [
                            {"const": "create"},
                            {"const": "update"}
                        ]
                    },
                    "tags": {
                        "type": "array",
                        "items": {"type": "string", "minLength": 1},
                        "minItems": 1,
                        "maxItems": 8
                    },
                    "metadata": {
                        "type": "object",
                        "properties": {
                            "trace": {
                                "type": "string",
                                "minLength": 2,
                                "maxLength": 64
                            },
                            "priority": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 10
                            }
                        }
                    }
                },
                "required": ["destination", "mode", "futureField"],
                "$defs": {
                    "__schema20": {
                        "type": "object",
                        "properties": {
                            "kind": {"enum": ["local", "thread"]},
                            "targetThreadId": {"type": ["string", "null"]}
                        },
                        "required": ["kind"]
                    }
                }
            }
        }],
        "stream": true,
        "max_output_tokens": 128
    })
}

#[test]
fn chat_request_policy_prepares_literal_production_wire_request() {
    let policy = CodexThirdPartyRequestPolicy::compile(&third_party_provider("openai_chat"))
        .expect("compile third-party request policy");

    let prepared = policy
        .prepare(
            CodexRequestTransport::ChatCompletions,
            logical_request(),
            CodexRequestOptions::default(),
        )
        .expect("prepare Chat request");

    assert_eq!(
        prepared.url,
        "https://gateway.example/custom/v1/chat/completions"
    );
    assert_eq!(
        prepared.headers[AUTHORIZATION],
        "Bearer request-policy-secret"
    );
    assert_eq!(prepared.headers[CONTENT_TYPE], "application/json");
    assert_eq!(prepared.headers[USER_AGENT], "CCSM-Contract/2");
    assert_eq!(prepared.headers["x-provider-policy"], "enabled");
    assert_ne!(prepared.headers[AUTHORIZATION], "Bearer forbidden");
    assert_eq!(prepared.body["model"], "upstream/model-v2");
    assert_eq!(prepared.body["reasoning_effort"], "xhigh");
    assert_eq!(prepared.body["max_tokens"], 128);
    assert_eq!(prepared.body["stream"], true);
    assert_eq!(prepared.body["metadata"]["probePolicy"], "shared");
    assert!(prepared.body.get("_privateProbeField").is_none());
}

#[test]
fn responses_request_policy_maps_effort_model_and_provider_overrides() {
    let policy = CodexThirdPartyRequestPolicy::compile(&third_party_provider("openai_responses"))
        .expect("compile third-party request policy");

    let prepared = policy
        .prepare(
            CodexRequestTransport::Responses,
            logical_request(),
            CodexRequestOptions::default(),
        )
        .expect("prepare Responses request");

    assert_eq!(prepared.url, "https://gateway.example/custom/v1/responses");
    assert_eq!(prepared.body["model"], "upstream/model-v2");
    assert_eq!(prepared.body["reasoning"]["effort"], "xhigh");
    assert_eq!(prepared.body["metadata"]["probePolicy"], "shared");
    assert_eq!(prepared.body["stream"], true);
    assert!(prepared.body.get("_privateProbeField").is_none());
    assert!(!policy.fingerprint().is_empty());
    assert!(!policy.credential_fingerprint().is_empty());

    let debug = format!("{policy:?}");
    assert!(!debug.contains("request-policy-secret"));
    assert!(!debug.contains("Bearer forbidden"));
}

#[test]
fn xai_responses_request_policy_applies_the_production_native_compatibility_layer() {
    let mut provider = third_party_provider("openai_responses");
    provider.settings_config["config"] = json!(
        r#"model = "grok-4.6"
model_provider = "xai"
[model_providers.xai]
base_url = "https://api.x.ai/v1"
wire_api = "responses"
"#
    );
    provider.settings_config["modelCatalog"] = json!({
        "models": [{"model": "grok-4.6", "upstreamModel": "grok-4.6"}]
    });
    let policy =
        CodexThirdPartyRequestPolicy::compile(&provider).expect("compile xAI request policy");

    let prepared = policy
        .prepare(
            CodexRequestTransport::Responses,
            json!({
                "model": "grok-4.6",
                "input": [{"role": "user", "content": [{"type": "input_text", "text": "probe"}]}],
                "prompt_cache_retention": "24h",
                "tools": [
                    {
                        "type": "custom",
                        "name": "apply_patch",
                        "description": "Apply a patch.",
                        "format": {"type": "grammar", "syntax": "lark", "definition": "start: /.+/"}
                    },
                    {
                        "type": "function",
                        "name": "dynamic_tool",
                        "parameters": {
                            "oneOf": [
                                {"type": "object", "properties": {"mode": {"const": "direct"}}, "required": ["mode"]},
                                {"type": "null"}
                            ]
                        }
                    }
                ]
            }),
            CodexRequestOptions::default(),
        )
        .expect("prepare xAI Responses request");

    assert!(prepared.body.get("prompt_cache_retention").is_none());
    assert_eq!(prepared.body["tools"][0]["type"], "function");
    assert_eq!(prepared.body["tools"][0]["parameters"]["type"], "object");
    assert_eq!(prepared.body["tools"][1]["parameters"]["type"], "object");
    assert!(prepared.body["tools"][1]["parameters"]
        .get("oneOf")
        .is_none());
}

#[test]
fn versioned_custom_api_root_does_not_gain_an_openai_v1_segment() {
    let mut provider = third_party_provider("openai_chat");
    provider.settings_config["config"] = json!(
        r#"model = "visible-model"
model_provider = "third-party"
[model_providers.third-party]
base_url = "https://open.bigmodel.cn/api/coding/paas/v4"
wire_api = "chat"
"#
    );
    let policy = CodexThirdPartyRequestPolicy::compile(&provider)
        .expect("compile versioned custom API request policy");

    assert_eq!(
        policy
            .prepare_url(CodexRequestTransport::ChatCompletions)
            .expect("prepare Chat endpoint"),
        "https://open.bigmodel.cn/api/coding/paas/v4/chat/completions"
    );
    assert_eq!(
        policy
            .prepare_url(CodexRequestTransport::Responses)
            .expect("prepare Responses endpoint"),
        "https://open.bigmodel.cn/api/coding/paas/v4/responses"
    );
}

#[test]
fn responses_request_policy_falls_back_unknown_codex_role_model_to_provider_default() {
    let mut provider = third_party_provider("openai_responses");
    provider.settings_config["config"] = json!(
        r#"model = "grok-4.6"
model_provider = "xai"
[model_providers.xai]
base_url = "https://api.x.ai/v1"
wire_api = "responses"
"#
    );
    provider.settings_config["modelCatalog"] = json!({
        "models": [
            {"model": "grok-4.6", "upstreamModel": "grok-4.6"},
            {"model": "grok-4.5", "upstreamModel": "grok-4.5"}
        ]
    });
    let policy =
        CodexThirdPartyRequestPolicy::compile(&provider).expect("compile xAI request policy");

    let prepared = policy
        .prepare_protocol_body(
            CodexRequestTransport::Responses,
            json!({"model": "gpt-5.6-sol", "input": "continue"}),
            &CodexRequestOptions::default(),
        )
        .expect("prepare native Responses body");

    assert_eq!(prepared["model"], "grok-4.6");
}

#[test]
fn managed_provider_cannot_compile_into_active_probe_policy() {
    let mut provider = third_party_provider("openai_responses");
    provider.meta.as_mut().unwrap().provider_type = Some("codex_oauth".to_string());

    assert!(CodexThirdPartyRequestPolicy::compile(&provider).is_err());
}

#[test]
fn openai_request_policy_preserves_standard_tool_schema_and_summary_replay() {
    let policy = CodexThirdPartyRequestPolicy::compile(&third_party_provider("openai_responses"))
        .expect("compile third-party request policy");
    let logical = realistic_codex_tool_request();
    let original_schema = logical["tools"][0]["parameters"].clone();

    let prepared = policy
        .prepare(
            CodexRequestTransport::Responses,
            logical,
            CodexRequestOptions::default(),
        )
        .expect("prepare strict OpenAI Responses request");

    assert_eq!(prepared.body["tools"][0]["parameters"], original_schema);
    assert_eq!(
        prepared.body["input"][0]["summary"][0]["text"],
        "Plan the next automation update."
    );
    assert_eq!(
        prepared.body["input"][0]["encrypted_content"],
        "opaque-official-replay"
    );
    assert!(prepared.body["input"][0].get("content").is_none());
}

#[test]
fn moonshot_request_policy_compiles_realistic_codex_schema_for_responses() {
    let policy = CodexThirdPartyRequestPolicy::compile(&third_party_provider("openai_responses"))
        .expect("compile third-party request policy");

    let prepared = policy
        .prepare(
            CodexRequestTransport::Responses,
            realistic_codex_tool_request(),
            CodexRequestOptions {
                tool_schema_dialect: Some(ToolSchemaDialect::MoonshotMfjs),
                ..CodexRequestOptions::default()
            },
        )
        .expect("compile tool schema into production Moonshot dialect");
    let schema = &prepared.body["tools"][0]["parameters"];

    assert!(schema.get("$defs").is_none());
    assert!(schema["properties"]["destination"].get("$ref").is_none());
    assert_eq!(schema["properties"]["destination"]["type"], "object");
    assert_eq!(
        schema["properties"]["destination"]["properties"]["kind"]["type"],
        "string"
    );
    assert!(schema["properties"]["mode"].get("oneOf").is_none());
    assert_eq!(
        schema["properties"]["mode"]["anyOf"][0]["enum"],
        json!(["create"])
    );
    assert_eq!(schema["properties"]["tags"]["minItems"], 1);
    assert_eq!(schema["properties"]["tags"]["maxItems"], 8);
    assert_eq!(schema["properties"]["tags"]["items"]["minLength"], 1);
    assert_eq!(
        schema["properties"]["metadata"]["properties"]["trace"]["minLength"],
        2
    );
    assert_eq!(
        schema["properties"]["metadata"]["properties"]["trace"]["maxLength"],
        64
    );
    assert_eq!(
        schema["properties"]["metadata"]["properties"]["priority"]["minimum"],
        1
    );
    assert_eq!(
        schema["properties"]["metadata"]["properties"]["priority"]["maximum"],
        10
    );
    assert_eq!(
        schema["properties"]["futureField"]["anyOf"]
            .as_array()
            .map(Vec::len),
        Some(6)
    );
}

#[test]
fn moonshot_schema_strips_unsupported_title_annotations_without_losing_supported_metadata() {
    let policy = CodexThirdPartyRequestPolicy::compile(&third_party_provider("openai_responses"))
        .expect("compile third-party request policy");
    let body = json!({
        "model": "visible-model",
        "input": "probe",
        "tools": [{
            "type": "function",
            "name": "find_workspace_file",
            "parameters": {
                "type": "object",
                "title": "Workspace query",
                "description": "Locate one file in the workspace.",
                "properties": {
                    "query": {
                        "type": "string",
                        "title": "Search text",
                        "description": "Text to find.",
                        "default": "README"
                    }
                },
                "required": ["query"]
            }
        }]
    });

    let prepared = policy
        .prepare(
            CodexRequestTransport::Responses,
            body,
            CodexRequestOptions {
                tool_schema_dialect: Some(ToolSchemaDialect::MoonshotMfjs),
                ..CodexRequestOptions::default()
            },
        )
        .expect("unsupported title annotations should be removed from Moonshot MFJS");
    let schema = &prepared.body["tools"][0]["parameters"];

    assert!(schema.get("title").is_none());
    assert!(schema["properties"]["query"].get("title").is_none());
    assert_eq!(schema["description"], "Locate one file in the workspace.");
    assert_eq!(
        schema["properties"]["query"]["description"],
        "Text to find."
    );
    assert_eq!(schema["properties"]["query"]["default"], "README");
}

#[test]
fn moonshot_schema_strips_comments_that_cannot_affect_tool_arguments() {
    let policy = CodexThirdPartyRequestPolicy::compile(&third_party_provider("openai_responses"))
        .expect("compile third-party request policy");
    let body = json!({
        "model": "visible-model",
        "input": "probe",
        "tools": [{
            "type": "function",
            "name": "read_workspace_file",
            "parameters": {
                "type": "object",
                "$comment": "Generated by a JSON Schema producer.",
                "properties": {
                    "path": {
                        "type": "string",
                        "$comment": "This annotation is not sent to the model.",
                        "description": "Absolute file path."
                    }
                },
                "required": ["path"]
            }
        }]
    });

    let prepared = policy
        .prepare(
            CodexRequestTransport::Responses,
            body,
            CodexRequestOptions {
                tool_schema_dialect: Some(ToolSchemaDialect::MoonshotMfjs),
                ..CodexRequestOptions::default()
            },
        )
        .expect("$comment should be removed because it has no instance semantics");
    let schema = &prepared.body["tools"][0]["parameters"];

    assert!(schema.get("$comment").is_none());
    assert!(schema["properties"]["path"].get("$comment").is_none());
    assert_eq!(
        schema["properties"]["path"]["description"],
        "Absolute file path."
    );
}

#[test]
fn moonshot_schema_projects_root_object_union_for_real_codex_dynamic_tools() {
    let policy = CodexThirdPartyRequestPolicy::compile(&third_party_provider("openai_responses"))
        .expect("compile third-party request policy");
    let body = json!({
        "model": "visible-model",
        "input": "probe",
        "tools": [{
            "type": "function",
            "name": "mcp__codex_app__automation_update",
            "strict": true,
            "parameters": {
                "oneOf": [
                    {
                        "type": "object",
                        "properties": {
                            "mode": {"const": "view"},
                            "id": {"type": "string"}
                        },
                        "required": ["mode", "id"],
                        "additionalProperties": false
                    },
                    {
                        "type": "object",
                        "properties": {
                            "mode": {"enum": ["create", "suggested_create"]},
                            "name": {"type": "string"},
                            "prompt": {"type": "string"}
                        },
                        "required": ["mode", "name", "prompt"],
                        "additionalProperties": false
                    },
                    {"type": "null"}
                ]
            }
        }]
    });

    let responses = policy
        .prepare(
            CodexRequestTransport::Responses,
            body.clone(),
            CodexRequestOptions {
                tool_schema_dialect: Some(ToolSchemaDialect::MoonshotMfjs),
                ..CodexRequestOptions::default()
            },
        )
        .expect("root object union should compile for strict Responses providers");
    let chat = policy
        .prepare(
            CodexRequestTransport::ChatCompletions,
            body,
            CodexRequestOptions {
                tool_schema_dialect: Some(ToolSchemaDialect::MoonshotMfjs),
                ..CodexRequestOptions::default()
            },
        )
        .expect("root object union should compile for strict Chat providers");
    let responses_tool = &responses.body["tools"][0];
    let responses_schema = &responses_tool["parameters"];
    let chat_tool = &chat.body["tools"][0]["function"];
    let chat_schema = &chat_tool["parameters"];

    assert_eq!(responses_schema, chat_schema);
    assert_eq!(responses_schema["type"], "object");
    assert!(responses_schema.get("oneOf").is_none());
    assert!(responses_schema.get("anyOf").is_none());
    assert_eq!(responses_schema["required"], json!(["mode"]));
    assert_eq!(
        responses_schema["properties"]["mode"]["anyOf"],
        json!([
            {"type": "string", "enum": ["view"]},
            {"type": "string", "enum": ["create", "suggested_create"]}
        ])
    );
    assert_eq!(responses_schema["additionalProperties"], false);
    assert_eq!(responses_tool["strict"], false);
    assert_eq!(chat_tool["strict"], false);
}

#[test]
fn moonshot_schema_compiles_combined_root_union_and_nested_ref_siblings() {
    let policy = CodexThirdPartyRequestPolicy::compile(&third_party_provider("openai_responses"))
        .expect("compile third-party request policy");
    let body = json!({
        "model": "visible-model",
        "input": "probe",
        "tools": [{
            "type": "function",
            "name": "codex_app__automation_update",
            "strict": true,
            "parameters": {
                "oneOf": [
                    {
                        "type": "object",
                        "properties": {
                            "mode": {"const": "view"},
                            "destination": {"$ref": "#/$defs/__schema20"}
                        },
                        "required": ["mode", "destination"],
                        "additionalProperties": false
                    },
                    {
                        "type": "object",
                        "properties": {
                            "mode": {"enum": ["create", "suggested_create"]},
                            "destination": {
                                "$ref": "#/$defs/__schema20",
                                "description": "Destination selected by the user."
                            }
                        },
                        "required": ["mode", "destination"],
                        "additionalProperties": false
                    },
                    {"type": "null"}
                ],
                "$defs": {
                    "__schema20": {
                        "$ref": "#/$defs/Destination",
                        "type": "object"
                    },
                    "Destination": {
                        "type": "object",
                        "properties": {
                            "kind": {"enum": ["local", "thread"]},
                            "targetThreadId": {"type": ["string", "null"]}
                        },
                        "required": ["kind"],
                        "additionalProperties": false
                    }
                }
            }
        }]
    });
    let options = CodexRequestOptions {
        tool_schema_dialect: Some(ToolSchemaDialect::MoonshotMfjs),
        ..CodexRequestOptions::default()
    };

    let responses = policy
        .prepare(
            CodexRequestTransport::Responses,
            body.clone(),
            options.clone(),
        )
        .expect("combined dynamic schema should compile for Responses");
    let chat = policy
        .prepare(CodexRequestTransport::ChatCompletions, body, options)
        .expect("combined dynamic schema should compile for Chat Completions");
    let responses_tool = &responses.body["tools"][0];
    let responses_schema = &responses_tool["parameters"];
    let chat_tool = &chat.body["tools"][0]["function"];
    let chat_schema = &chat_tool["parameters"];

    assert_eq!(responses_schema, chat_schema);
    assert_eq!(responses_schema["type"], "object");
    assert!(responses_schema.get("oneOf").is_none());
    assert!(responses_schema.get("anyOf").is_none());
    assert!(responses_schema.get("$defs").is_none());
    assert!(!responses_schema.to_string().contains("\"$ref\""));
    let destination_variants = responses_schema["properties"]["destination"]["anyOf"]
        .as_array()
        .expect("both destination branches remain available");
    assert_eq!(destination_variants.len(), 2);
    assert!(destination_variants.iter().all(|variant| {
        variant["type"] == "object"
            && variant["properties"]["kind"]["type"] == "string"
            && variant["properties"]["targetThreadId"]["anyOf"].is_array()
    }));
    assert_eq!(responses_tool["strict"], false);
    assert_eq!(chat_tool["strict"], false);
}

#[test]
fn moonshot_schema_compilation_is_identical_after_chat_tool_shape_conversion() {
    let policy = CodexThirdPartyRequestPolicy::compile(&third_party_provider("openai_chat"))
        .expect("compile third-party request policy");
    let responses = policy
        .prepare(
            CodexRequestTransport::Responses,
            realistic_codex_tool_request(),
            CodexRequestOptions {
                tool_schema_dialect: Some(ToolSchemaDialect::MoonshotMfjs),
                ..CodexRequestOptions::default()
            },
        )
        .expect("prepare Responses request");
    let chat = policy
        .prepare(
            CodexRequestTransport::ChatCompletions,
            realistic_codex_tool_request(),
            CodexRequestOptions {
                tool_schema_dialect: Some(ToolSchemaDialect::MoonshotMfjs),
                ..CodexRequestOptions::default()
            },
        )
        .expect("prepare Chat request");

    assert_eq!(
        chat.body["tools"][0]["function"]["parameters"],
        responses.body["tools"][0]["parameters"]
    );
}

#[test]
fn reasoning_text_replay_is_opt_in_for_responses_dialect() {
    let policy = CodexThirdPartyRequestPolicy::compile(&third_party_provider("openai_responses"))
        .expect("compile third-party request policy");

    let prepared = policy
        .prepare(
            CodexRequestTransport::Responses,
            realistic_codex_tool_request(),
            CodexRequestOptions {
                history_replay: Some(HistoryReplay::ResponsesReasoningTextContent),
                ..CodexRequestOptions::default()
            },
        )
        .expect("prepare DeepSeek-style replay request");
    let replay = &prepared.body["input"][0];

    assert_eq!(
        replay["content"],
        json!([{
            "type": "reasoning_text",
            "text": "Plan the next automation update."
        }])
    );
    assert_eq!(
        replay["summary"],
        json!([]),
        "strict Responses replay keeps the required summary field empty instead of relabeling raw reasoning as a summary"
    );
    assert!(replay.get("encrypted_content").is_none());
}

#[test]
fn moonshot_schema_requires_an_object_root_even_when_codex_omits_root_type() {
    let policy = CodexThirdPartyRequestPolicy::compile(&third_party_provider("openai_responses"))
        .expect("compile third-party request policy");
    let body = json!({
        "model": "visible-model",
        "input": "probe",
        "tools": [{
            "type": "function",
            "name": "search_workspace",
            "parameters": {
                "properties": {
                    "query": {"type": "string"}
                },
                "required": ["query"]
            }
        }]
    });

    let prepared = policy
        .prepare(
            CodexRequestTransport::Responses,
            body,
            CodexRequestOptions {
                tool_schema_dialect: Some(ToolSchemaDialect::MoonshotMfjs),
                ..CodexRequestOptions::default()
            },
        )
        .expect("compile an object-shaped Codex tool for Moonshot");
    let schema = &prepared.body["tools"][0]["parameters"];

    assert_eq!(schema["type"], "object");
    assert_eq!(schema["properties"]["query"]["type"], "string");
    assert!(schema.get("anyOf").is_none());
}

#[test]
fn moonshot_schema_rejects_unrepresentable_composition_instead_of_dropping_it() {
    let policy = CodexThirdPartyRequestPolicy::compile(&third_party_provider("openai_responses"))
        .expect("compile third-party request policy");
    let body = json!({
        "model": "visible-model",
        "input": "probe",
        "tools": [{
            "type": "function",
            "name": "merge_guarded_settings",
            "parameters": {
                "type": "object",
                "allOf": [
                    {"properties": {"source": {"type": "string"}}},
                    {"properties": {"accountId": {"type": "string"}}}
                ]
            }
        }]
    });

    let error = policy
        .prepare(
            CodexRequestTransport::Responses,
            body,
            CodexRequestOptions {
                tool_schema_dialect: Some(ToolSchemaDialect::MoonshotMfjs),
                ..CodexRequestOptions::default()
            },
        )
        .expect_err("MFJS cannot safely preserve allOf intersection semantics");
    let message = error.to_string();

    assert!(message.contains("merge_guarded_settings"));
    assert!(message.contains("$.tools[0].parameters"));
    assert!(message.contains("allOf"));
}

#[test]
fn moonshot_schema_rejects_semantic_constraints_and_tuple_items_instead_of_loosening() {
    let policy = CodexThirdPartyRequestPolicy::compile(&third_party_provider("openai_responses"))
        .expect("compile third-party request policy");
    for (tool_name, property_schema, expected_keyword) in [
        (
            "pattern_guard",
            json!({"type": "string", "pattern": "^[a-z]+$"}),
            "pattern",
        ),
        (
            "tuple_guard",
            json!({"type": "array", "items": [{"type": "string"}, {"type": "integer"}]}),
            "tuple",
        ),
    ] {
        let body = json!({
            "model": "visible-model",
            "input": "probe",
            "tools": [{
                "type": "function",
                "name": tool_name,
                "parameters": {
                    "type": "object",
                    "properties": {"value": property_schema}
                }
            }]
        });

        let error = policy
            .prepare(
                CodexRequestTransport::Responses,
                body,
                CodexRequestOptions {
                    tool_schema_dialect: Some(ToolSchemaDialect::MoonshotMfjs),
                    ..CodexRequestOptions::default()
                },
            )
            .expect_err("unsupported MFJS constructs must fail closed");
        let message = error.to_string();
        assert!(message.contains(tool_name));
        assert!(message.contains(expected_keyword), "{message}");
    }
}

#[test]
fn moonshot_schema_drops_format_annotations_and_disables_strict_validation() {
    let policy = CodexThirdPartyRequestPolicy::compile(&third_party_provider("openai_responses"))
        .expect("compile third-party request policy");
    let body = json!({
        "model": "visible-model",
        "input": "probe",
        "tools": [{
            "type": "function",
            "name": "format_annotation",
            "strict": true,
            "parameters": {
                "type": "object",
                "properties": {
                    "host": {"type": "string", "format": "hostname", "minLength": 1}
                },
                "required": ["host"]
            }
        }]
    });

    let prepared = policy
        .prepare(
            CodexRequestTransport::Responses,
            body,
            CodexRequestOptions {
                tool_schema_dialect: Some(ToolSchemaDialect::MoonshotMfjs),
                ..CodexRequestOptions::default()
            },
        )
        .expect("format is an annotation and can be removed without changing instance shape");
    let tool = &prepared.body["tools"][0];

    assert!(tool["parameters"]["properties"]["host"]
        .get("format")
        .is_none());
    assert_eq!(tool["parameters"]["properties"]["host"]["minLength"], 1);
    assert_eq!(tool["strict"], false);
}

#[test]
fn moonshot_schema_rejects_overlapping_one_of_instead_of_changing_exclusive_semantics() {
    let policy = CodexThirdPartyRequestPolicy::compile(&third_party_provider("openai_responses"))
        .expect("compile third-party request policy");
    let body = json!({
        "model": "visible-model",
        "input": "probe",
        "tools": [{
            "type": "function",
            "name": "exclusive_choice",
            "parameters": {
                "type": "object",
                "properties": {
                    "value": {
                        "oneOf": [
                            {"type": "number"},
                            {"type": "integer"}
                        ]
                    }
                }
            }
        }]
    });

    let error = policy
        .prepare(
            CodexRequestTransport::Responses,
            body,
            CodexRequestOptions {
                tool_schema_dialect: Some(ToolSchemaDialect::MoonshotMfjs),
                ..CodexRequestOptions::default()
            },
        )
        .expect_err("overlapping oneOf branches cannot become anyOf");

    assert!(error.to_string().contains("oneOf"));
}

#[test]
fn moonshot_schema_merges_supported_ref_and_any_of_constraints_without_losing_them() {
    let policy = CodexThirdPartyRequestPolicy::compile(&third_party_provider("openai_responses"))
        .expect("compile third-party request policy");
    let body = json!({
        "model": "visible-model",
        "input": "probe",
        "tools": [{
            "type": "function",
            "name": "bounded_choice",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": {
                        "$ref": "#/$defs/name",
                        "minLength": 5
                    },
                    "code": {
                        "type": "string",
                        "minLength": 3,
                        "anyOf": [
                            {"maxLength": 8},
                            {"enum": ["fallback"]}
                        ]
                    }
                },
                "$defs": {
                    "name": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": 50
                    }
                }
            }
        }]
    });

    let prepared = policy
        .prepare(
            CodexRequestTransport::Responses,
            body,
            CodexRequestOptions {
                tool_schema_dialect: Some(ToolSchemaDialect::MoonshotMfjs),
                ..CodexRequestOptions::default()
            },
        )
        .expect("supported MFJS bounds can be merged exactly");
    let properties = &prepared.body["tools"][0]["parameters"]["properties"];

    assert_eq!(properties["name"]["type"], "string");
    assert_eq!(properties["name"]["minLength"], 5);
    assert_eq!(properties["name"]["maxLength"], 50);
    assert_eq!(properties["code"]["anyOf"][0]["type"], "string");
    assert_eq!(properties["code"]["anyOf"][0]["minLength"], 3);
    assert_eq!(properties["code"]["anyOf"][0]["maxLength"], 8);
    assert_eq!(properties["code"]["anyOf"][1]["type"], "string");
    assert_eq!(properties["code"]["anyOf"][1]["minLength"], 3);
    assert_eq!(properties["code"]["anyOf"][1]["enum"], json!(["fallback"]));
}

#[test]
fn moonshot_schema_compiles_recursive_local_refs_and_rejects_remote_refs() {
    let policy = CodexThirdPartyRequestPolicy::compile(&third_party_provider("openai_responses"))
        .expect("compile third-party request policy");
    let recursive = json!({
        "model": "visible-model",
        "input": "probe",
        "tools": [{
            "type": "function",
            "name": "walk_tree",
            "parameters": {
                "type": "object",
                "properties": {
                    "root": {"$ref": "#/$defs/node"}
                },
                "required": ["root"],
                "$defs": {
                    "node": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"},
                            "children": {
                                "type": "array",
                                "items": {"$ref": "#/$defs/node"}
                            }
                        },
                        "required": ["name", "children"]
                    }
                }
            }
        }]
    });
    let prepared = policy
        .prepare(
            CodexRequestTransport::Responses,
            recursive,
            CodexRequestOptions {
                tool_schema_dialect: Some(ToolSchemaDialect::MoonshotMfjs),
                ..CodexRequestOptions::default()
            },
        )
        .expect("compile recursive local refs into root-level MFJS definitions");
    let schema = &prepared.body["tools"][0]["parameters"];
    let definitions = schema["$defs"]
        .as_object()
        .expect("recursive aliases live at the schema root");

    assert_eq!(definitions.len(), 1);
    let recursive_ref = definitions
        .values()
        .next()
        .and_then(|definition| definition.pointer("/properties/children/items/$ref"))
        .and_then(Value::as_str)
        .expect("recursive child keeps a local MFJS ref");
    assert!(recursive_ref.starts_with("#/$defs/"));

    let remote = json!({
        "model": "visible-model",
        "input": "probe",
        "tools": [{
            "type": "function",
            "name": "remote_lookup",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {"$ref": "https://schemas.example/query.json"}
                }
            }
        }]
    });
    let error = policy
        .prepare(
            CodexRequestTransport::Responses,
            remote,
            CodexRequestOptions {
                tool_schema_dialect: Some(ToolSchemaDialect::MoonshotMfjs),
                ..CodexRequestOptions::default()
            },
        )
        .expect_err("remote refs must fail locally before reaching Moonshot");
    let message = error.to_string();

    assert!(message.contains("remote_lookup"));
    assert!(message.contains("$.tools[0].parameters.properties.query"));
    assert!(message.contains("remote $ref"));
}

#[test]
fn provider_override_tools_are_validated_after_the_final_body_merge() {
    let mut provider = third_party_provider("openai_responses");
    provider
        .meta
        .as_mut()
        .expect("provider meta")
        .local_proxy_request_overrides
        .as_mut()
        .expect("provider overrides")
        .body = Some(json!({
        "additional_tools": [{
            "namespace": "settings",
            "tools": [{
                "type": "function",
                "name": "save_provider",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "mode": {"const": "save", "pattern": "^save$"}
                    },
                    "required": ["mode"]
                }
            }]
        }]
    }));
    let policy = CodexThirdPartyRequestPolicy::compile(&provider)
        .expect("compile third-party request policy");

    let error = policy
        .finalize_body(
            CodexRequestTransport::Responses,
            json!({"model": "visible-model", "input": "probe"}),
            &CodexRequestOptions {
                tool_schema_dialect: Some(ToolSchemaDialect::MoonshotMfjs),
                ..CodexRequestOptions::default()
            },
        )
        .expect_err("Provider override tools must pass the same MFJS validation");

    assert!(error.to_string().contains("save_provider"));
    assert!(error.to_string().contains("pattern"));
}

#[test]
fn moonshot_schema_rejects_required_property_forbidden_by_additional_properties() {
    let provider = third_party_provider("openai_responses");
    let policy = CodexThirdPartyRequestPolicy::compile(&provider)
        .expect("compile third-party request policy");
    let request = json!({
        "model": "visible-model",
        "input": "probe",
        "tools": [{
            "type": "function",
            "name": "closed_required_property",
            "parameters": {
                "type": "object",
                "properties": {},
                "required": ["missing"],
                "additionalProperties": false
            }
        }]
    });

    let error = policy
        .finalize_body(
            CodexRequestTransport::Responses,
            request,
            &CodexRequestOptions {
                tool_schema_dialect: Some(ToolSchemaDialect::MoonshotMfjs),
                ..CodexRequestOptions::default()
            },
        )
        .expect_err("a required property forbidden by the source schema must fail closed");

    assert!(error.to_string().contains("closed_required_property"));
    assert!(error.to_string().contains("required"));
    assert!(error.to_string().contains("additionalProperties"));
}

#[test]
fn moonshot_schema_preserves_required_property_additional_properties_constraint() {
    let provider = third_party_provider("openai_responses");
    let policy = CodexThirdPartyRequestPolicy::compile(&provider)
        .expect("compile third-party request policy");
    let request = json!({
        "model": "visible-model",
        "input": "probe",
        "tools": [{
            "type": "function",
            "name": "typed_required_property",
            "parameters": {
                "type": "object",
                "required": ["payload"],
                "additionalProperties": {
                    "type": "string",
                    "minLength": 2
                }
            }
        }]
    });

    let prepared = policy
        .finalize_body(
            CodexRequestTransport::Responses,
            request,
            &CodexRequestOptions {
                tool_schema_dialect: Some(ToolSchemaDialect::MoonshotMfjs),
                ..CodexRequestOptions::default()
            },
        )
        .expect("the additionalProperties schema can be copied exactly");

    assert_eq!(
        prepared["tools"][0]["parameters"]["properties"]["payload"],
        json!({"type": "string", "minLength": 2})
    );
}
