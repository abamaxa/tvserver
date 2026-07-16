use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use app_lib::domain::messages::{Command, PlayRequest};
use roas::validation::{Options as RoasOptions, Validate as RoasValidate};
use serde_json::Value;

const CONTRACT_PATH: &str = "docs/api/openapi.yaml";

fn contract() -> Value {
    let source = fs::read_to_string(CONTRACT_PATH)
        .unwrap_or_else(|error| panic!("failed to read {CONTRACT_PATH}: {error}"));
    let specification = oas3::from_yaml(source.as_str())
        .unwrap_or_else(|error| panic!("{CONTRACT_PATH} failed typed OpenAPI parsing: {error}"));
    specification
        .validate_version()
        .unwrap_or_else(|error| panic!("{CONTRACT_PATH} has an unsupported version: {error}"));
    let yaml: serde_yaml::Value = serde_yaml::from_str(&source)
        .unwrap_or_else(|error| panic!("failed to parse {CONTRACT_PATH} as YAML: {error}"));
    serde_json::to_value(yaml).expect("OpenAPI YAML should convert to a JSON value")
}

#[test]
fn roas_semantically_validates_the_openapi_31_document() {
    let document = contract();
    let specification: roas::v3_1::spec::Spec =
        serde_json::from_value(document).expect("roas must deserialize the OpenAPI 3.1 document");
    specification
        .validate(RoasOptions::empty(), None)
        .unwrap_or_else(|error| panic!("roas semantic validation failed:\n{error}"));
}

fn resolve_local_reference<'a>(document: &'a Value, reference: &str) -> Option<&'a Value> {
    let pointer = reference.strip_prefix('#')?;
    document.pointer(pointer)
}

fn visit_references(value: &Value, visit: &mut impl FnMut(&str)) {
    match value {
        Value::Object(object) => {
            if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
                visit(reference);
            }
            for child in object.values() {
                visit_references(child, visit);
            }
        }
        Value::Array(array) => {
            for child in array {
                visit_references(child, visit);
            }
        }
        _ => {}
    }
}

fn template_parameters(path: &str) -> Vec<&str> {
    path.split('{')
        .skip(1)
        .filter_map(|part| part.split_once('}').map(|(parameter, _)| parameter))
        .collect()
}

fn resolved<'a>(document: &'a Value, value: &'a Value) -> &'a Value {
    value
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|reference| resolve_local_reference(document, reference))
        .unwrap_or(value)
}

fn schema_accepts(document: &Value, schema: &Value, instance: &Value) -> bool {
    let schema = resolved(document, schema);

    if let Some(variants) = schema.get("anyOf").and_then(Value::as_array) {
        return variants
            .iter()
            .any(|variant| schema_accepts(document, variant, instance));
    }

    if let Some(variants) = schema.get("oneOf").and_then(Value::as_array) {
        return variants
            .iter()
            .filter(|variant| schema_accepts(document, variant, instance))
            .count()
            == 1;
    }

    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        if !values.contains(instance) {
            return false;
        }
    }

    let matches_type = |schema_type: &str| match schema_type {
        "array" => instance.is_array(),
        "boolean" => instance.is_boolean(),
        "integer" => instance.as_i64().is_some() || instance.as_u64().is_some(),
        "null" => instance.is_null(),
        "number" => instance.is_number(),
        "object" => instance.is_object(),
        "string" => instance.is_string(),
        _ => false,
    };
    if let Some(schema_type) = schema.get("type") {
        let type_matches = match schema_type {
            Value::String(schema_type) => matches_type(schema_type),
            Value::Array(schema_types) => schema_types
                .iter()
                .filter_map(Value::as_str)
                .any(matches_type),
            _ => false,
        };
        if !type_matches {
            return false;
        }
    }

    if let Some(object) = instance.as_object() {
        let properties = schema.get("properties").and_then(Value::as_object);
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            if required
                .iter()
                .filter_map(Value::as_str)
                .any(|field| !object.contains_key(field))
            {
                return false;
            }
        }
        if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
            let Some(properties) = properties else {
                return object.is_empty();
            };
            if object.keys().any(|field| !properties.contains_key(field)) {
                return false;
            }
        }
        if let Some(properties) = properties {
            for (field, value) in object {
                if let Some(property_schema) = properties.get(field) {
                    if !schema_accepts(document, property_schema, value) {
                        return false;
                    }
                }
            }
        }
    }

    if let (Some(items), Some(array)) = (schema.get("items"), instance.as_array()) {
        if array
            .iter()
            .any(|item| !schema_accepts(document, items, item))
        {
            return false;
        }
    }

    true
}

fn valid_component_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || ".-_".contains(character))
}

fn valid_response_code(code: &str) -> bool {
    if code == "default" {
        return true;
    }
    let bytes = code.as_bytes();
    bytes.len() == 3
        && matches!(bytes[0], b'1'..=b'5')
        && ((bytes[1].is_ascii_digit() && bytes[2].is_ascii_digit())
            || (bytes[1] == b'X' && bytes[2] == b'X'))
}

fn validate_content_schemas(errors: &mut Vec<String>, context: &str, content: &Value) {
    let Some(content) = content.as_object() else {
        errors.push(format!("{context} content must be an object"));
        return;
    };
    if content.is_empty() {
        errors.push(format!("{context} content must not be empty"));
    }
    for (media_type, media) in content {
        if media.get("schema").and_then(Value::as_object).is_none() {
            errors.push(format!("{context} media type {media_type} must define a schema"));
        }
    }
}

fn semantic_contract_errors(document: &Value) -> Vec<String> {
    const METHODS: &[&str] = &["get", "put", "post", "delete", "options", "head", "patch", "trace"];

    let mut errors = Vec::new();
    let mut operation_ids = BTreeSet::new();
    let Some(paths) = document.get("paths").and_then(Value::as_object) else {
        return vec!["paths must be an object".to_string()];
    };

    if let Some(components) = document.get("components").and_then(Value::as_object) {
        for (component_type, entries) in components {
            let Some(entries) = entries.as_object() else {
                errors.push(format!("components.{component_type} must be an object"));
                continue;
            };
            for key in entries.keys() {
                if !valid_component_key(key) {
                    errors.push(format!("invalid component key {component_type}.{key}"));
                }
            }
        }
    }

    for (path, path_item) in paths {
        let Some(path_item) = path_item.as_object() else {
            errors.push(format!("path item {path} must be an object"));
            continue;
        };
        let template_parameters: BTreeSet<_> = template_parameters(path)
            .into_iter()
            .map(str::to_string)
            .collect();

        for method in METHODS {
            let Some(operation) = path_item.get(*method) else {
                continue;
            };
            let context = format!("{} {}", method.to_ascii_uppercase(), path);
            let Some(operation) = operation.as_object() else {
                errors.push(format!("{context} operation must be an object"));
                continue;
            };

            match operation.get("operationId").and_then(Value::as_str) {
                Some(operation_id) if !operation_id.is_empty() => {
                    if !operation_ids.insert(operation_id.to_string()) {
                        errors.push(format!("duplicate operationId {operation_id}"));
                    }
                }
                _ => errors.push(format!("{context} must define a non-empty operationId")),
            }

            let mut declared_path_parameters = BTreeSet::new();
            for parameter in path_item
                .get("parameters")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .chain(
                    operation
                        .get("parameters")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten(),
                )
            {
                let parameter = resolved(document, parameter);
                if parameter.get("in").and_then(Value::as_str) != Some("path") {
                    continue;
                }
                let Some(name) = parameter.get("name").and_then(Value::as_str) else {
                    errors.push(format!("{context} has a path parameter without a name"));
                    continue;
                };
                if !declared_path_parameters.insert(name.to_string()) {
                    errors.push(format!("{context} repeats path parameter {name}"));
                }
                if parameter.get("required") != Some(&Value::Bool(true)) {
                    errors.push(format!("{context} path parameter {name} must be required"));
                }
                if parameter.get("allowReserved").is_some() {
                    errors.push(format!(
                        "{context} path parameter {name} must not use query-only allowReserved"
                    ));
                }
            }
            if declared_path_parameters != template_parameters {
                errors.push(format!(
                    "{context} path parameters {:?} do not exactly match template {:?}",
                    declared_path_parameters, template_parameters
                ));
            }

            if let Some(request_body) = operation.get("requestBody") {
                let request_body = resolved(document, request_body);
                match request_body.get("content") {
                    Some(content) => validate_content_schemas(
                        &mut errors,
                        &format!("{context} request body"),
                        content,
                    ),
                    None => errors.push(format!("{context} request body must define content")),
                }
            }

            let Some(responses) = operation.get("responses").and_then(Value::as_object) else {
                errors.push(format!("{context} must define a responses object"));
                continue;
            };
            if responses.is_empty() {
                errors.push(format!("{context} responses must not be empty"));
            }
            for (code, response) in responses {
                if !valid_response_code(code) {
                    errors.push(format!("{context} has invalid response code {code}"));
                }
                let response = resolved(document, response);
                if response
                    .get("description")
                    .and_then(Value::as_str)
                    .is_none()
                {
                    errors.push(format!("{context} response {code} must have a description"));
                }
                if let Some(content) = response.get("content") {
                    validate_content_schemas(
                        &mut errors,
                        &format!("{context} response {code}"),
                        content,
                    );
                }
            }
        }
    }

    errors
}

#[test]
fn openapi_contract_typed_parses_and_meets_project_requirements() {
    assert!(
        Path::new(CONTRACT_PATH).is_file(),
        "the canonical OpenAPI contract must exist at {CONTRACT_PATH}"
    );
    let document = contract();

    assert_eq!(document["openapi"], "3.1.0");
    assert!(document["info"]["title"].is_string());
    assert!(document["info"]["version"].is_string());
    let paths = document["paths"]
        .as_object()
        .expect("paths must be an object");
    let schemas = document["components"]["schemas"]
        .as_object()
        .expect("components.schemas must be an object");

    let required_operations: BTreeMap<&str, &[&str]> = BTreeMap::from([
        ("/api/book-thumbnails/{file}", &["get"] as &[_]),
        ("/api/book/{checksum}", &["get", "delete"]),
        ("/api/books", &["get"]),
        ("/api/books/download/{path}", &["get"]),
        ("/api/books/{collection}", &["get"]),
        ("/api/control/ws", &["get"]),
        ("/api/conversion", &["get"]),
        ("/api/log", &["post"]),
        ("/api/media", &["get"]),
        ("/api/media/{media}", &["get", "delete", "post", "patch"]),
        ("/api/remote", &["get"]),
        ("/api/remote/control", &["post"]),
        ("/api/remote/play", &["post"]),
        ("/api/remote/ws", &["get"]),
        ("/api/search/pirate", &["get"]),
        ("/api/search/youtube", &["get"]),
        ("/api/stream-audio/{audio_index}/{path}", &["get"]),
        ("/api/stream/{path}", &["get"]),
        ("/api/tasks", &["get", "post"]),
        ("/api/tasks/{type}/{path}", &["delete"]),
        ("/api/thumbnails/{path}", &["get"]),
    ]);

    for (path, methods) in required_operations {
        let path_item = paths
            .get(path)
            .unwrap_or_else(|| panic!("missing required path {path}"));
        for method in methods {
            let operation = path_item
                .get(method)
                .unwrap_or_else(|| panic!("missing {method} operation for {path}"));
            assert!(
                operation["operationId"].is_string(),
                "{method} {path} must define operationId"
            );
            assert!(
                operation["responses"].is_object(),
                "{method} {path} must define responses"
            );

            for parameter_name in template_parameters(path) {
                let parameters = operation["parameters"]
                    .as_array()
                    .unwrap_or_else(|| panic!("{method} {path} must define path parameters"));
                let parameter = parameters.iter().find(|parameter| {
                    let parameter = parameter
                        .get("$ref")
                        .and_then(Value::as_str)
                        .and_then(|reference| resolve_local_reference(&document, reference))
                        .unwrap_or(parameter);
                    parameter["in"] == "path" && parameter["name"] == parameter_name
                });
                let parameter = parameter.unwrap_or_else(|| {
                    panic!("{method} {path} is missing path parameter {parameter_name}")
                });
                let parameter = parameter
                    .get("$ref")
                    .and_then(Value::as_str)
                    .and_then(|reference| resolve_local_reference(&document, reference))
                    .unwrap_or(parameter);
                assert_eq!(parameter["required"], true);
            }
        }
    }

    for schema in [
        "Response",
        "ErrorResponse",
        "TaskState",
        "TaskListResults",
        "SearchResults",
        "CollectionItem",
        "CollectionDetails",
        "MediaItem",
        "VideoDetails",
        "BookMetadata",
        "BookCollectionItem",
        "BookCollectionDetails",
        "BookDetails",
    ] {
        assert!(schemas.contains_key(schema), "missing reusable schema {schema}");
    }

    let mut references = Vec::new();
    visit_references(&document, &mut |reference| {
        references.push(reference.to_string())
    });
    assert!(
        !references.is_empty(),
        "contract should reuse component references"
    );
    for reference in references {
        assert!(
            reference.starts_with("#/"),
            "external reference {reference} is not self-contained"
        );
        assert!(
            resolve_local_reference(&document, &reference).is_some(),
            "unresolved OpenAPI reference {reference}"
        );
    }

    assert_eq!(
        schemas["BookDetails"]["properties"]["checksum"]["type"],
        "string"
    );
    assert_eq!(
        schemas["VideoDetails"]["properties"]["checksum"]["type"],
        "string"
    );

    for (path, methods, parameter_name) in [
        ("/api/tasks/{type}/{path}", &["delete"] as &[_], "path"),
        (
            "/api/media/{media}",
            &["get", "delete", "post", "patch"] as &[_],
            "media",
        ),
        ("/api/books/{collection}", &["get"] as &[_], "collection"),
        ("/api/books/download/{path}", &["get"] as &[_], "path"),
        (
            "/api/stream-audio/{audio_index}/{path}",
            &["get"] as &[_],
            "path",
        ),
        ("/api/stream/{path}", &["get"] as &[_], "path"),
        ("/api/thumbnails/{path}", &["get"] as &[_], "path"),
    ] {
        for method in methods {
            let operation = paths[path][method]
                .as_object()
                .expect("wildcard route must have an operation");
            let parameter = operation["parameters"]
                .as_array()
                .expect("wildcard route must define parameters")
                .iter()
                .find(|parameter| parameter["name"] == parameter_name)
                .expect("wildcard parameter must be documented");
            let description = parameter["description"]
                .as_str()
                .expect("wildcard parameter must have a description")
                .to_ascii_lowercase();
            assert!(description.contains("url-encoded"));
            assert!(description.contains("nested path"));
        }
    }

    let media_schema = serde_json::to_string(&schemas["MediaItem"]).unwrap();
    assert!(media_schema.contains("VideoDetails"));
    assert!(media_schema.contains("CollectionDetails"));
    assert!(
        !media_schema.to_ascii_lowercase().contains("book"),
        "/api/media's MediaItem schema must remain video-only"
    );
    assert_eq!(
        paths["/api/media"]["get"]["responses"]["200"]["content"]["application/json"]["schema"]
            ["$ref"],
        "#/components/schemas/MediaItem"
    );
}

#[test]
fn serve_dir_thumbnail_route_is_greedy_and_path_parameters_are_standards_compliant() {
    let document = contract();
    let paths = document["paths"].as_object().unwrap();
    assert!(paths.contains_key("/api/thumbnails/{path}"));
    assert!(!paths.contains_key("/api/thumbnails/{file}"));

    let parameter = &paths["/api/thumbnails/{path}"]["get"]["parameters"][0];
    let description = parameter["description"]
        .as_str()
        .unwrap()
        .to_ascii_lowercase();
    assert!(description.contains("greedy nested path"));
    assert!(description.contains("url-encoded"));
    assert!(description.contains("decoded"));

    let mut path_parameters_with_allow_reserved = Vec::new();
    fn find_invalid_path_parameters(value: &Value, invalid: &mut Vec<String>) {
        match value {
            Value::Object(object) => {
                if object.get("in").and_then(Value::as_str) == Some("path")
                    && object.contains_key("allowReserved")
                {
                    invalid.push(
                        object
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("<unnamed>")
                            .to_string(),
                    );
                }
                for child in object.values() {
                    find_invalid_path_parameters(child, invalid);
                }
            }
            Value::Array(array) => {
                for child in array {
                    find_invalid_path_parameters(child, invalid);
                }
            }
            _ => {}
        }
    }
    find_invalid_path_parameters(&document, &mut path_parameters_with_allow_reserved);
    assert!(
        path_parameters_with_allow_reserved.is_empty(),
        "path parameters must not use query-only allowReserved: {:?}",
        path_parameters_with_allow_reserved
    );
}

#[test]
fn remote_request_schemas_accept_runtime_valid_serde_payloads() {
    let document = contract();
    let schemas = document["components"]["schemas"].as_object().unwrap();

    let command_fixtures = [
        serde_json::json!({
            "message": {
                "State": {
                    "collection": "Series",
                    "video": "episode.webm",
                    "videoId": "42",
                    "paused": false
                }
            }
        }),
        serde_json::json!({
            "message": {
                "State": {
                    "currentTime": null,
                    "duration": null,
                    "collection": "Series",
                    "video": "episode.webm",
                    "videoId": "42",
                    "paused": false,
                    "currentSubtitleTrack": null
                }
            }
        }),
        serde_json::json!({"message": {"SetSubtitleTrack": {}}}),
        serde_json::json!({
            "message": {
                "Play": {
                    "videoId": "42",
                    "url": "/api/stream/episode.webm",
                    "collection": "",
                    "video": "episode.webm",
                    "width": 1920,
                    "height": 1080,
                    "aspectWidth": 16,
                    "aspectHeight": 9
                }
            }
        }),
        serde_json::json!({
            "message": {
                "Play": {
                    "videoId": "42",
                    "url": "/api/stream/episode.webm",
                    "collection": "",
                    "video": "episode.webm",
                    "width": 1920,
                    "height": 1080,
                    "aspectWidth": 16,
                    "aspectHeight": 9,
                    "metadata": null
                }
            }
        }),
        serde_json::json!({"message": {"SetSubtitleTrack": {"trackId": null}}}),
    ];
    for fixture in command_fixtures {
        serde_json::from_value::<Command>(fixture.clone())
            .expect("fixture must be accepted by runtime Command deserialization");
        assert!(
            schema_accepts(&document, &schemas["Command"], &fixture),
            "Command schema rejected runtime-valid fixture: {fixture}"
        );
    }

    let play_request = serde_json::json!({
        "collection": "",
        "video": "episode.webm",
        "width": 1920,
        "height": 1080,
        "aspectWidth": 16,
        "aspectHeight": 9,
        "videoId": "42"
    });
    serde_json::from_value::<PlayRequest>(play_request.clone())
        .expect("fixture must be accepted by runtime PlayRequest deserialization");
    assert!(
        schema_accepts(&document, &schemas["PlayRequest"], &play_request),
        "PlayRequest schema rejected runtime-valid fixture"
    );

    let play_request_with_null_metadata = serde_json::json!({
        "collection": "",
        "video": "episode.webm",
        "width": 1920,
        "height": 1080,
        "aspectWidth": 16,
        "aspectHeight": 9,
        "videoId": "42",
        "metadata": null
    });
    serde_json::from_value::<PlayRequest>(play_request_with_null_metadata.clone())
        .expect("null metadata must be accepted by runtime PlayRequest deserialization");
    assert!(schema_accepts(
        &document,
        &schemas["PlayRequest"],
        &play_request_with_null_metadata
    ));
}

#[test]
fn inbound_last_state_schema_accepts_defaulted_video_details() {
    let document = contract();
    let command_schema = &document["components"]["schemas"]["Command"];
    let fixtures = [
        serde_json::json!({"message": {"LastState": {}}}),
        serde_json::json!({
            "message": {
                "LastState": {
                    "video": "episode.mkv",
                    "checksum": "42"
                }
            }
        }),
        serde_json::json!({
            "ignoredCommandField": true,
            "message": {
                "LastState": {
                    "ignoredVideoField": true,
                    "series": {
                        "seriesTitle": "Example",
                        "season": "1",
                        "episode": "2",
                        "episodeTitle": "Nested defaults",
                        "ignoredSeriesField": true
                    },
                    "metadata": {
                        "duration": 120.0,
                        "width": 1920,
                        "height": 1080,
                        "aspectWidth": 16,
                        "aspectHeight": 9,
                        "audioTracks": 1,
                        "audioTrackList": [{
                            "id": 0,
                            "language": "eng",
                            "title": null,
                            "ignoredTrackField": true
                        }],
                        "subtitleTracks": null,
                        "ignoredMetadataField": true
                    }
                }
            }
        }),
    ];

    for fixture in fixtures {
        serde_json::from_value::<Command>(fixture.clone())
            .expect("runtime Command deserialization accepts defaulted VideoDetails");
        assert!(
            schema_accepts(&document, command_schema, &fixture),
            "inbound Command schema rejected runtime-valid LastState: {fixture}"
        );
    }
}

#[test]
fn audio_stream_index_matches_the_runtime_u32_extractor() {
    let document = contract();
    let parameters = document["paths"]["/api/stream-audio/{audio_index}/{path}"]["get"]
        ["parameters"]
        .as_array()
        .unwrap();
    let audio_index = parameters
        .iter()
        .find(|parameter| parameter["name"] == "audio_index")
        .expect("audio stream route must document audio_index");
    assert_eq!(audio_index["schema"]["minimum"], 0);
    assert_eq!(audio_index["schema"]["maximum"], 4_294_967_295_u64);
}

#[test]
fn security_contract_matches_restrict_access_router_layering() {
    const METHODS: &[&str] = &["get", "put", "post", "delete", "options", "head", "patch", "trace"];
    const UNPROTECTED_PATHS: &[&str] = &[
        "/api/stream/{path}",
        "/api/stream-audio/{audio_index}/{path}",
        "/api/thumbnails/{path}",
        "/api/book-thumbnails/{file}",
    ];

    let document = contract();
    let security_schemes = document["components"]["securitySchemes"]
        .as_object()
        .expect("restrict_access authentication schemes must be documented");
    assert_eq!(security_schemes["HttpBasicAuth"]["type"], "http");
    assert_eq!(security_schemes["HttpBasicAuth"]["scheme"], "basic");
    assert_eq!(security_schemes["QueryAuth"]["type"], "apiKey");
    assert_eq!(security_schemes["QueryAuth"]["in"], "query");
    assert_eq!(security_schemes["QueryAuth"]["name"], "auth");
    for scheme in ["HttpBasicAuth", "QueryAuth"] {
        let description = security_schemes[scheme]["description"]
            .as_str()
            .expect("authentication schemes need runtime-grounded descriptions");
        assert!(description.contains("AUTH_CREDENTIALS"));
    }

    let security = document["security"]
        .as_array()
        .expect("protected routes must inherit security requirements");
    assert_eq!(security.len(), 2);
    assert!(!security
        .iter()
        .any(|requirement| requirement == &serde_json::json!({})));
    assert!(security
        .iter()
        .any(|requirement| requirement == &serde_json::json!({"HttpBasicAuth": []})));
    assert!(security
        .iter()
        .any(|requirement| requirement == &serde_json::json!({"QueryAuth": []})));
    let auth_description = document["info"]["description"]
        .as_str()
        .unwrap()
        .to_ascii_lowercase();
    assert!(auth_description.contains("localhost"));
    assert!(auth_description.contains("192.168"));
    assert!(auth_description.contains("x-real-ip"));
    assert!(auth_description.contains("x-forwarded-for"));

    let unauthorized = &document["components"]["responses"]["UnauthorizedResponse"];
    assert_eq!(
        unauthorized["headers"]["WWW-Authenticate"]["schema"]["type"],
        "string"
    );
    assert_eq!(
        unauthorized["headers"]["WWW-Authenticate"]["example"],
        "Basic realm=\"tvserver\""
    );

    for (path, path_item) in document["paths"].as_object().unwrap() {
        for method in METHODS {
            let Some(operation) = path_item.get(*method) else {
                continue;
            };
            if UNPROTECTED_PATHS.contains(&path.as_str()) {
                assert_eq!(
                    operation["security"],
                    serde_json::json!([]),
                    "{method} {path} must override global security because its router is unprotected"
                );
                assert!(operation["responses"].get("401").is_none());
            } else {
                assert_eq!(
                    operation["responses"]["401"]["$ref"],
                    "#/components/responses/UnauthorizedResponse",
                    "{method} {path} is layered with restrict_access and must document 401"
                );
            }
        }
    }
}

#[test]
fn stream_responses_cover_all_video_content_types() {
    let document = contract();
    for status in ["200", "206"] {
        let content = document["paths"]["/api/stream/{path}"]["get"]["responses"][status]
            ["content"]
            .as_object()
            .unwrap();
        let content_types: BTreeSet<_> = content.keys().map(String::as_str).collect();
        assert_eq!(
            content_types,
            BTreeSet::from(["video/*", "application/octet-stream"]),
            "stream response {status} must cover every ServeDir video MIME plus its fallback"
        );
    }
}

#[test]
fn search_query_schema_allows_the_empty_string_accepted_by_the_handler() {
    let document = contract();
    let query = &document["components"]["parameters"]["SearchQuery"]["schema"];
    assert!(
        query.get("minLength").and_then(Value::as_u64).unwrap_or(0) == 0,
        "the handler accepts q=, so the contract must not require a non-empty query"
    );
}

#[test]
fn contract_passes_project_semantic_openapi_checks() {
    let document = contract();
    let errors = semantic_contract_errors(&document);
    assert!(
        errors.is_empty(),
        "OpenAPI semantic validation failed:\n{}",
        errors.join("\n")
    );
}

#[test]
fn project_semantic_checks_reject_representative_contract_defects() {
    let mut document = contract();

    document["paths"]["/api/books"]["get"]["operationId"] = Value::String("listTasks".to_string());
    document["paths"]["/api/books/{collection}"]["get"]["parameters"] = Value::Array(Vec::new());
    document["paths"]["/api/log"]["post"]["responses"]["200"]
        .as_object_mut()
        .unwrap()
        .remove("description");
    let invalid_response = document["paths"]["/api/log"]["post"]["responses"]["200"].clone();
    document["paths"]["/api/log"]["post"]["responses"]
        .as_object_mut()
        .unwrap()
        .insert("099".to_string(), invalid_response);
    document["paths"]["/api/tasks"]["post"]["requestBody"]["content"]["application/json"]
        .as_object_mut()
        .unwrap()
        .remove("schema");
    let response_schema = document["components"]["schemas"]["Response"].clone();
    document["components"]["schemas"]
        .as_object_mut()
        .unwrap()
        .insert("invalid component key".to_string(), response_schema);

    let errors = semantic_contract_errors(&document).join("\n");
    for expected in [
        "duplicate operationId listTasks",
        "path parameters {} do not exactly match template {\"collection\"}",
        "response 200 must have a description",
        "invalid response code 099",
        "media type application/json must define a schema",
        "invalid component key schemas.invalid component key",
    ] {
        assert!(
            errors.contains(expected),
            "semantic checks did not report {expected:?}; errors were:\n{errors}"
        );
    }
}
