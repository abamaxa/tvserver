use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    net::SocketAddr,
    path::Path,
};

use app_lib::domain::messages::{
    ClientLogMessage, Command, ConversionRequest, DownloadRequest, PlayRequest, RemoteMessage,
};
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

    if let (Some(pattern), Some(string)) =
        (schema.get("pattern").and_then(Value::as_str), instance.as_str())
    {
        if !regex::Regex::new(pattern)
            .expect("OpenAPI patterns must be valid regular expressions")
            .is_match(string)
        {
            return false;
        }
    }

    if let (Some(min_length), Some(string)) = (
        schema.get("minLength").and_then(Value::as_u64),
        instance.as_str(),
    ) {
        if string.chars().count() < min_length as usize {
            return false;
        }
    }

    if schema.get("format").and_then(Value::as_str) == Some("date-time") {
        let Some(string) = instance.as_str() else {
            return false;
        };
        if chrono::DateTime::parse_from_rfc3339(string).is_err() {
            return false;
        }
    }

    if schema.get("format").and_then(Value::as_str) == Some("socket-address") {
        let Some(string) = instance.as_str() else {
            return false;
        };
        if string.parse::<SocketAddr>().is_err() {
            return false;
        }
    }

    if let Some(number) = instance.as_f64() {
        if schema
            .get("minimum")
            .and_then(Value::as_f64)
            .is_some_and(|minimum| number < minimum)
        {
            return false;
        }
        if schema
            .get("maximum")
            .and_then(Value::as_f64)
            .is_some_and(|maximum| number > maximum)
        {
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

#[test]
fn book_progress_schema_helper_enforces_scalar_constraints() {
    let document = serde_json::json!({});

    let bounded_number = serde_json::json!({"type": "number", "minimum": 0, "maximum": 1});
    assert!(schema_accepts(&document, &bounded_number, &serde_json::json!(0)));
    assert!(schema_accepts(&document, &bounded_number, &serde_json::json!(1)));
    assert!(!schema_accepts(
        &document,
        &bounded_number,
        &serde_json::json!(-0.01)
    ));
    assert!(!schema_accepts(
        &document,
        &bounded_number,
        &serde_json::json!(1.01)
    ));

    let nonempty_string = serde_json::json!({"type": "string", "minLength": 1});
    assert!(schema_accepts(
        &document,
        &nonempty_string,
        &serde_json::json!("x")
    ));
    assert!(!schema_accepts(
        &document,
        &nonempty_string,
        &serde_json::json!("")
    ));

    let timestamp = serde_json::json!({"type": "string", "format": "date-time"});
    assert!(schema_accepts(
        &document,
        &timestamp,
        &serde_json::json!("2026-07-19T12:00:00Z")
    ));
    assert!(!schema_accepts(
        &document,
        &timestamp,
        &serde_json::json!("2026-07-19 12:00:00")
    ));
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

fn validate_content_schemas(
    errors: &mut Vec<String>,
    context: &str,
    content: &Value,
    allow_empty_media_type_object: bool,
) {
    let Some(content) = content.as_object() else {
        errors.push(format!("{context} content must be an object"));
        return;
    };
    if content.is_empty() {
        errors.push(format!("{context} content must not be empty"));
    }
    for (media_type, media) in content {
        if !media.is_object() {
            errors.push(format!(
                "{context} media type {media_type} must be a Media Type Object"
            ));
        } else if media.get("schema").and_then(Value::as_object).is_none()
            && !(allow_empty_media_type_object
                && media.as_object().is_some_and(serde_json::Map::is_empty))
        {
            errors.push(format!("{context} media type {media_type} must define a schema"));
        }
    }
}

fn allows_empty_response_media_type(path: &str, method: &str, status: &str) -> bool {
    matches!(
        (path, method, status),
        ("/api/books/download/{path}", "get", "200" | "206" | "416")
            | ("/api/book-thumbnails/{file}", "get", "200")
            | ("/api/stream/{path}", "get", "200" | "206" | "416")
            | ("/api/stream-audio/{audio_index}/{path}", "get", "200")
            | ("/api/thumbnails/{path}", "get", "200" | "206" | "416")
    )
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
                        false,
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
                        allows_empty_response_media_type(path, method, code),
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
fn book_progress_is_nested_and_only_put_is_documented() {
    let document = contract();
    let path = &document["paths"]["/api/book/{checksum}/progress"];
    assert!(path.get("get").is_none());
    assert!(path.get("delete").is_none());
    assert!(path["put"]["responses"].get("204").is_some());
    assert!(document["paths"].get("/api/book-progress").is_none());
    assert_eq!(
        document["components"]["schemas"]["BookDetails"]["properties"]["progress"]["$ref"],
        "#/components/schemas/BookReadingProgress"
    );
    assert!(
        document["components"]["schemas"]["BookReadingProgress"]["properties"]
            .get("checksum")
            .is_none()
    );
}

#[test]
fn book_progress_operations_document_runtime_responses_and_payload_ownership() {
    let document = contract();
    let paths = document["paths"].as_object().unwrap();
    let schemas = document["components"]["schemas"].as_object().unwrap();

    for schema in [
        "BookReadingProgress",
        "SaveBookProgressRequest",
        "BookLocator",
        "EpubCfiLocator",
        "PdfPageLocator",
    ] {
        assert!(schemas.contains_key(schema), "missing reusable schema {schema}");
    }

    let expected_responses = [(
        "/api/book/{checksum}/progress",
        "put",
        &["204", "400", "401", "404", "500", "503"] as &[_],
    )];
    for (path, method, expected) in expected_responses {
        let operation = &paths[path][method];
        assert!(operation["operationId"].is_string(), "missing {method} {path}");
        let actual: BTreeSet<_> = operation["responses"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(actual, expected.iter().copied().collect(), "{method} {path}");

        for status in ["400", "404", "500", "503"] {
            if operation["responses"].get(status).is_some() {
                let response = resolved(&document, &operation["responses"][status]);
                assert_eq!(
                    response["content"]["application/json"]["schema"]["$ref"],
                    "#/components/schemas/ErrorResponse",
                    "{method} {path} response {status} must use the JSON error shape"
                );
            }
        }
        assert_eq!(
            operation["responses"]["401"]["$ref"],
            "#/components/responses/UnauthorizedResponse"
        );
    }

    assert!(paths.get("/api/book-progress").is_none());
    assert!(paths["/api/book/{checksum}/progress"].get("get").is_none());
    assert!(paths["/api/book/{checksum}/progress"]
        .get("delete")
        .is_none());
    assert!(paths["/api/book/{checksum}/progress"]["put"]["responses"]["204"]
        .get("content")
        .is_none());
    assert_eq!(
        paths["/api/book/{checksum}/progress"]["put"]["requestBody"]["content"]["application/json"]
            ["schema"]["$ref"],
        "#/components/schemas/SaveBookProgressRequest"
    );
    let request = &schemas["SaveBookProgressRequest"];
    assert_eq!(request["additionalProperties"], false);
    assert_eq!(request["required"], serde_json::json!(["locator"]));
    let request_fields: BTreeSet<_> = request["properties"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(request_fields, BTreeSet::from(["locator", "progression"]));

    let response = &schemas["BookReadingProgress"];
    assert_eq!(response["additionalProperties"], false);
    assert_eq!(response["required"], serde_json::json!(["locator", "updatedOn"]));
    assert_eq!(response["properties"]["updatedOn"]["format"], "date-time");
    assert!(response["properties"].get("checksum").is_none());
    assert_eq!(
        schemas["BookDetails"]["properties"]["progress"]["$ref"],
        "#/components/schemas/BookReadingProgress"
    );
}

#[test]
fn book_progress_schemas_accept_valid_epub_and_pdf_payloads() {
    let document = contract();
    let schemas = document["components"]["schemas"].as_object().unwrap();
    let epub_request = serde_json::json!({
        "locator": {"type": "epub-cfi", "value": "epubcfi(/6/4!/4/2/8)"},
        "progression": 0.0
    });
    let pdf_request = serde_json::json!({
        "locator": {"type": "pdf-page", "value": "opaque-page-token"},
        "progression": 1.0
    });
    let null_progression_request = serde_json::json!({
        "locator": {"type": "pdf-page", "value": "7"},
        "progression": null
    });
    for request in [&epub_request, &pdf_request, &null_progression_request] {
        assert!(
            schema_accepts(&document, &schemas["SaveBookProgressRequest"], request),
            "valid save request was rejected: {request}"
        );
    }

    let epub_response = serde_json::json!({
        "locator": {"type": "epub-cfi", "value": "epubcfi(/6/4!/4/2/8)"},
        "progression": 0.42,
        "updatedOn": "2026-07-19T12:00:00Z"
    });
    let pdf_response = serde_json::json!({
        "locator": {"type": "pdf-page", "value": "7"},
        "updatedOn": "2026-07-19T12:00:00.000Z"
    });
    for response in [&epub_response, &pdf_response] {
        assert!(response["updatedOn"].as_str().unwrap().ends_with('Z'));
        assert!(
            schema_accepts(&document, &schemas["BookReadingProgress"], response),
            "valid progress response was rejected: {response}"
        );
    }
}

#[test]
fn book_progress_schemas_reject_invalid_payloads() {
    let document = contract();
    let schemas = document["components"]["schemas"].as_object().unwrap();
    let valid_request = serde_json::json!({
        "locator": {"type": "pdf-page", "value": "1"},
        "progression": 0.5
    });
    let valid_response = serde_json::json!({
        "locator": {"type": "epub-cfi", "value": "epubcfi(/6/2)"},
        "progression": 0.5,
        "updatedOn": "2026-07-19T12:00:00Z"
    });

    for invalid in [
        serde_json::json!({"locator": {"type": "future", "value": "1"}}),
        serde_json::json!({"locator": {"type": "pdf-page", "value": ""}}),
        serde_json::json!({"locator": {"type": "pdf-page", "value": " \t"}}),
        serde_json::json!({"locator": {"type": "pdf-page", "value": "1", "extra": true}}),
        serde_json::json!({"locator": {"type": "pdf-page", "value": "1"}, "progression": -0.01}),
        serde_json::json!({"locator": {"type": "pdf-page", "value": "1"}, "progression": 1.01}),
        serde_json::json!({"locator": {"type": "pdf-page", "value": "1"}, "checksum": "1"}),
        serde_json::json!({"locator": {"type": "pdf-page", "value": "1"}, "updatedOn": "2026-07-19T12:00:00Z"}),
    ] {
        assert!(
            !schema_accepts(&document, &schemas["SaveBookProgressRequest"], &invalid),
            "invalid save request was accepted: {invalid}"
        );
    }

    for field in ["locator", "updatedOn"] {
        let mut invalid = valid_response.clone();
        invalid.as_object_mut().unwrap().remove(field);
        assert!(
            !schema_accepts(&document, &schemas["BookReadingProgress"], &invalid),
            "response without server-owned {field} was accepted"
        );
    }
    for (field, value) in [
        ("checksum", serde_json::json!("9223372036854775807")),
        ("progression", serde_json::json!(null)),
        ("progression", serde_json::json!(-0.01)),
        ("progression", serde_json::json!(1.01)),
        ("updatedOn", serde_json::json!("2026-07-19 12:00:00")),
        ("updatedOn", serde_json::json!("2026-07-19T12:00:00+02:00")),
    ] {
        let mut invalid = valid_response.clone();
        invalid[field] = value;
        assert!(
            !schema_accepts(&document, &schemas["BookReadingProgress"], &invalid),
            "invalid response field {field} was accepted: {invalid}"
        );
    }
    assert!(schema_accepts(
        &document,
        &schemas["SaveBookProgressRequest"],
        &valid_request
    ));
}

#[test]
fn book_progress_checksum_parameter_uses_the_bounded_signed_string_schema() {
    let document = contract();
    let checksum = resolved(
        &document,
        &document["components"]["parameters"]["BookChecksum"]["schema"],
    );

    for valid in [
        serde_json::json!("0"),
        serde_json::json!("-1"),
        serde_json::json!("9223372036854775807"),
        serde_json::json!("-9223372036854775808"),
    ] {
        assert!(schema_accepts(&document, checksum, &valid), "rejected {valid}");
    }
    for invalid in [
        serde_json::json!(1),
        serde_json::json!("9223372036854775808"),
        serde_json::json!("-9223372036854775809"),
        serde_json::json!("1.0"),
    ] {
        assert!(
            !schema_accepts(&document, checksum, &invalid),
            "accepted invalid checksum {invalid}"
        );
    }
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
        serde_json::json!({"message": {"Command": {"command": "stop", "ignored": true}}}),
        serde_json::json!({"message": {"Seek": {"interval": 30, "ignored": true}}}),
        serde_json::json!({"message": {"SetAudioTrack": {"trackId": 1, "ignored": true}}}),
        serde_json::json!({"message": {"SetPlaybackRate": {"rate": 1.25, "ignored": true}}}),
        serde_json::json!({
            "message": {
                "State": {
                    "collection": "Series",
                    "video": "episode.webm",
                    "videoId": "42",
                    "paused": false,
                    "ignored": true
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
                    "ignored": true
                }
            }
        }),
        serde_json::json!({"message": {"SetSubtitleTrack": {"ignored": true}}}),
        serde_json::json!({
            "message": {
                "CurrentTasks": [{
                    "key": "task-1",
                    "name": "download",
                    "displayName": "Download",
                    "finished": false,
                    "eta": 10,
                    "percentDone": 50.0,
                    "sizeDetails": "1 MB",
                    "rateDetails": "1 MB/s",
                    "processDetails": "running",
                    "errorString": "",
                    "taskType": "Transmission",
                    "ignored": true
                }]
            }
        }),
        serde_json::json!({
            "message": {
                "Book": [{
                    "type": "BookEventAdded",
                    "book": {"ignoredBookField": true},
                    "checksum": "42",
                    "ignoredEventField": true
                }]
            }
        }),
        serde_json::json!({
            "message": {
                "Video": [{
                    "type": "VideoEventAdded",
                    "video": {"ignoredVideoField": true},
                    "checksum": "42",
                    "ignoredEventField": true
                }]
            }
        }),
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

    for (schema_name, fixture) in [
        (
            "DownloadRequest",
            serde_json::json!({
                "name": "Example",
                "link": "magnet:?xt=urn:example",
                "engine": "Torrent",
                "series": null,
                "ignored": true
            }),
        ),
        (
            "ClientLogMessage",
            serde_json::json!({"level": "info", "messages": ["hello"], "ignored": true}),
        ),
        (
            "ConversionRequest",
            serde_json::json!({"name": "mobile", "ignored": true}),
        ),
    ] {
        match schema_name {
            "DownloadRequest" => {
                serde_json::from_value::<DownloadRequest>(fixture.clone()).unwrap();
            }
            "ClientLogMessage" => {
                serde_json::from_value::<ClientLogMessage>(fixture.clone()).unwrap();
            }
            "ConversionRequest" => {
                serde_json::from_value::<ConversionRequest>(fixture.clone()).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            schema_accepts(&document, &schemas[schema_name], &fixture),
            "{schema_name} schema rejected a runtime-valid unknown property"
        );
    }

    let pong_schema = schemas["RemoteMessage"]["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|variant| variant.pointer("/properties/Pong"))
        .expect("RemoteMessage must define its Pong payload");
    assert_eq!(pong_schema["format"], "socket-address");
    for valid_address in
        ["127.0.0.1:0", "255.255.255.255:65535", "[::1]:8080", "[2001:db8::1]:65535"]
    {
        let valid_pong = serde_json::json!({"Pong": valid_address});
        serde_json::from_value::<RemoteMessage>(valid_pong.clone())
            .expect("runtime accepts a valid socket address");
        assert!(
            schema_accepts(&document, &schemas["RemoteMessageRequest"], &valid_pong),
            "Pong input schema must accept valid SocketAddr {valid_address}"
        );
    }

    for invalid_address in
        ["not-an-address", "999.999.999.999:8080", "127.0.0.1:99999", "[::::]:8080"]
    {
        let invalid_pong = serde_json::json!({"Pong": invalid_address});
        assert!(serde_json::from_value::<RemoteMessage>(invalid_pong.clone()).is_err());
        assert!(
            !schema_accepts(&document, &schemas["RemoteMessageRequest"], &invalid_pong),
            "Pong input schema must reject invalid SocketAddr {invalid_address}"
        );
    }
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
fn raw_file_responses_use_openapi_31_binary_media_types() {
    let document = contract();
    for (path, status) in [
        ("/api/books/download/{path}", "200"),
        ("/api/books/download/{path}", "206"),
        ("/api/books/download/{path}", "416"),
        ("/api/book-thumbnails/{file}", "200"),
        ("/api/stream/{path}", "200"),
        ("/api/stream/{path}", "206"),
        ("/api/stream-audio/{audio_index}/{path}", "200"),
        ("/api/thumbnails/{path}", "200"),
        ("/api/thumbnails/{path}", "206"),
        ("/api/stream/{path}", "416"),
        ("/api/thumbnails/{path}", "416"),
    ] {
        let response = resolved(&document, &document["paths"][path]["get"]["responses"][status]);
        let content = response["content"]
            .as_object()
            .unwrap_or_else(|| panic!("{path} response {status} must define content"));
        for (media_type, media) in content {
            assert_eq!(
                media,
                &serde_json::json!({}),
                "raw {media_type} response {status} for {path} must use an empty Media Type Object"
            );
        }
    }
}

#[test]
fn serve_dir_contract_documents_runtime_responses_and_headers() {
    let document = contract();
    for path in ["/api/stream/{path}", "/api/thumbnails/{path}"] {
        let responses = &document["paths"][path]["get"]["responses"];
        for status in ["200", "206", "304", "307", "404", "412", "416", "500"] {
            assert!(
                responses.get(status).is_some(),
                "ServeDir route {path} must document status {status}"
            );
        }

        for status in ["200", "206"] {
            let response = resolved(&document, &responses[status]);
            let content = response["content"].as_object().unwrap();
            assert_eq!(
                content,
                &serde_json::Map::from_iter([("*/*".to_string(), serde_json::json!({}))]),
                "ServeDir guesses MIME from any served file extension"
            );
            for header in ["Accept-Ranges", "Content-Length", "Last-Modified"] {
                assert!(response["headers"].get(header).is_some());
            }
        }
        assert!(resolved(&document, &responses["206"])["headers"]
            .get("Content-Range")
            .is_some());
        assert!(resolved(&document, &responses["307"])["headers"]
            .get("Location")
            .is_some());
        assert!(resolved(&document, &responses["416"])["headers"]
            .get("Content-Range")
            .is_some());
    }
}

#[test]
fn book_download_contract_documents_byte_ranges() {
    let document = contract();
    let operation = &document["paths"]["/api/books/download/{path}"]["get"];
    let parameters = operation["parameters"].as_array().unwrap();
    assert!(parameters
        .iter()
        .any(|parameter| { parameter["name"] == "Range" && parameter["in"] == "header" }));

    let responses = &operation["responses"];
    for status in ["200", "206", "304", "412", "416"] {
        assert!(
            responses.get(status).is_some(),
            "book download route must document status {status}"
        );
    }
    for status in ["200", "206", "416"] {
        let response = resolved(&document, &responses[status]);
        assert_eq!(response["headers"]["Accept-Ranges"]["schema"]["const"], "bytes");
    }
    for status in ["200", "206"] {
        let response = resolved(&document, &responses[status]);
        assert!(response["headers"].get("Content-Length").is_some());
        assert!(response["headers"].get("Last-Modified").is_some());
    }
    assert_eq!(
        responses["304"]["$ref"],
        "#/components/responses/ServeDirNotModifiedResponse"
    );
    assert_eq!(
        responses["412"]["$ref"],
        "#/components/responses/ServeDirPreconditionFailedResponse"
    );
    assert!(resolved(&document, &responses["206"])["headers"]
        .get("Content-Range")
        .is_some());
    assert!(resolved(&document, &responses["416"])["headers"]
        .get("Content-Range")
        .is_some());
}

#[test]
fn axum_extractor_rejections_are_documented_as_plain_text() {
    let document = contract();
    let expected_components = [
        ("400", "#/components/responses/BadRequestTextResponse"),
        ("413", "#/components/responses/PayloadTooLargeTextResponse"),
        ("415", "#/components/responses/UnsupportedMediaTypeTextResponse"),
        ("422", "#/components/responses/UnprocessableEntityTextResponse"),
    ];

    for (path, method) in [
        ("/api/tasks", "post"),
        ("/api/log", "post"),
        ("/api/media/{media}", "post"),
        ("/api/remote/control", "post"),
        ("/api/remote/play", "post"),
    ] {
        for (status, reference) in expected_components {
            assert_eq!(
                document["paths"][path][method]["responses"][status]["$ref"], reference,
                "{method} {path} must document Axum JSON rejection {status}"
            );
        }
    }

    for (path, method) in [
        ("/api/tasks/{type}/{path}", "delete"),
        ("/api/media/{media}", "get"),
        ("/api/media/{media}", "delete"),
        ("/api/media/{media}", "patch"),
        ("/api/books/{collection}", "get"),
        ("/api/books/download/{path}", "get"),
        ("/api/book-thumbnails/{file}", "get"),
        ("/api/stream-audio/{audio_index}/{path}", "get"),
    ] {
        assert_eq!(
            document["paths"][path][method]["responses"]["400"]["$ref"],
            "#/components/responses/BadRequestTextResponse",
            "{method} {path} must document Axum path rejection"
        );
    }

    for (_, reference) in expected_components {
        let response = resolve_local_reference(&document, reference).unwrap();
        assert_eq!(response["content"]["text/plain"]["schema"]["type"], "string");
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
    document["paths"]["/api/media"]["get"]["responses"]["200"]["content"]["application/json"]
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
        "GET /api/media response 200 media type application/json must define a schema",
        "invalid component key schemas.invalid component key",
    ] {
        assert!(
            errors.contains(expected),
            "semantic checks did not report {expected:?}; errors were:\n{errors}"
        );
    }
}
