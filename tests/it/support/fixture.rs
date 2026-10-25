use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use sego::Reply;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawRequest {
    pub method: String,
    pub path: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fault {
    Network,
    Redirect,
    HtmlChallenge,
    Status,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variables: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<RawRequest>,
    #[serde(default = "ok")]
    pub status: u16,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fault: Option<Fault>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub handwritten: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<Value>,
    #[serde(skip)]
    pub file: PathBuf,
}

fn ok() -> u16 {
    200
}

impl Default for Fixture {
    fn default() -> Fixture {
        Fixture {
            operation: None,
            variables: None,
            query: None,
            request: None,
            status: ok(),
            headers: BTreeMap::new(),
            fault: None,
            pinned: false,
            handwritten: false,
            body_file: None,
            response: None,
            file: PathBuf::new(),
        }
    }
}

const MAX_NODES: usize = 10_000_000;

impl Fixture {
    pub fn parse(text: &str, file: &Path) -> Result<Fixture, String> {
        let mut budget = serde_saphyr::Budget::default();
        budget.max_nodes = MAX_NODES;
        let mut options = serde_saphyr::Options::default();
        options.budget = Some(budget);
        let mut fixture: Fixture =
            serde_saphyr::from_str_with_options(text, options)
                .map_err(|e| e.to_string())?;
        fixture.file = file.to_path_buf();
        match (&fixture.operation, &fixture.request) {
            (Some(_), None) => {}
            (None, Some(request)) if request.method == "GET" => {}
            (None, Some(_)) => return Err("request.method must be GET".into()),
            _ => {
                return Err("exactly one of operation and request".into());
            }
        }
        if fixture.response.is_some() && fixture.body_file.is_some() {
            return Err("response and body_file are exclusive".into());
        }
        Ok(fixture)
    }

    pub fn to_yaml(&self) -> String {
        serde_saphyr::to_string(self).unwrap_or_else(|e| panic!("{e}"))
    }

    /// A string response is served as its bytes; any other response as JSON.
    pub fn body(&self) -> Vec<u8> {
        if let Some(name) = &self.body_file {
            let path = self.file.with_file_name(name);
            return fs::read(&path)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        }
        match &self.response {
            None => Vec::new(),
            Some(Value::String(text)) => text.clone().into_bytes(),
            Some(value) => value.to_string().into_bytes(),
        }
    }

    pub fn key(&self) -> String {
        match (&self.operation, &self.request) {
            (Some(operation), _) => format!(
                "{operation} {}",
                self.variables.as_ref().unwrap_or(&Value::Null)
            ),
            (None, Some(request)) => {
                format!("{} {}", request.method, request.path)
            }
            (None, None) => String::new(),
        }
    }
}

pub fn load_all(root: &Path) -> Vec<Fixture> {
    let mut files = Vec::new();
    collect(root, &mut files);
    files.sort();
    let mut seen: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut fixtures = Vec::new();
    for file in files {
        let text = fs::read_to_string(&file)
            .unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        let fixture = Fixture::parse(&text, &file)
            .unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        if let Some(first) = seen.insert(fixture.key(), file.clone()) {
            panic!(
                "duplicate fixture {}: {} and {}",
                fixture.key(),
                first.display(),
                file.display()
            );
        }
        fixtures.push(fixture);
    }
    fixtures
}

fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    for entry in entries {
        let path = entry.unwrap_or_else(|e| panic!("{e}")).path();
        if path.is_dir() {
            collect(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "yaml") {
            files.push(path);
        }
    }
}

pub fn reply(name: &str) -> Reply {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{name}.yaml"));
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let fixture = Fixture::parse(&text, &path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let header = |name: &str| fixture.headers.get(name).cloned();
    Reply {
        status: fixture.status,
        content_type: header("content-type"),
        location: header("location"),
        retry_after: header("retry-after"),
        cf_mitigated: header("cf-mitigated"),
        body: fixture.body(),
    }
}
