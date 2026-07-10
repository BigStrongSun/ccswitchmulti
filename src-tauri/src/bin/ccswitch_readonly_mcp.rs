use rmcp::{
    model::{
        ErrorData, ListResourceTemplatesResult, ListResourcesResult, PaginatedRequestParams,
        ReadResourceRequestParams, ReadResourceResult, Resource, ResourceContents,
        ResourceTemplate, ServerCapabilities, ServerInfo,
    },
    service::RequestContext,
    transport::stdio,
    RoleServer, ServerHandler, ServiceExt,
};
use std::{
    collections::VecDeque,
    env, fs,
    path::{Component, Path, PathBuf},
};

const TREE_URI: &str = "ccswitch://project/tree";
const TREE_URI_PREFIX: &str = "ccswitch://project/tree/";
const FILE_URI_PREFIX: &str = "ccswitch://project/file/";
const ROOT_TREE_DEPTH: usize = 2;
const SUBTREE_DEPTH: usize = 4;
const MAX_TREE_ENTRIES: usize = 2000;
const MAX_FILE_BYTES: u64 = 512 * 1024;
const IGNORED_DIRS: &[&str] = &[".git", "node_modules", "target", "dist", "build"];

#[derive(Debug, Clone)]
struct ReadonlyProjectServer {
    root: PathBuf,
}

impl ReadonlyProjectServer {
    fn from_env() -> anyhow::Result<Self> {
        let root = env::var("CCSWITCH_READONLY_ROOT")
            .map_err(|_| anyhow::anyhow!("CCSWITCH_READONLY_ROOT is not set"))?;
        let root = fs::canonicalize(root)?;
        if !root.is_dir() {
            anyhow::bail!("CCSWITCH_READONLY_ROOT is not a directory");
        }
        Ok(Self { root })
    }

    fn tree_text(&self, uri: &str) -> Result<String, ErrorData> {
        let (base, max_depth) = if uri == TREE_URI {
            (self.root.clone(), ROOT_TREE_DEPTH)
        } else if let Some(rel) = uri.strip_prefix(TREE_URI_PREFIX) {
            (resolve_inside_root(&self.root, rel)?, SUBTREE_DEPTH)
        } else {
            return Err(ErrorData::resource_not_found("resource not found", None));
        };
        if !base.is_dir() {
            return Err(invalid_params("resource is not a directory"));
        }

        let mut out = String::new();
        out.push_str(&format!("root: {}\n", self.root.display()));
        let rel_base = base.strip_prefix(&self.root).unwrap_or(&base);
        let scope = if rel_base.as_os_str().is_empty() {
            ".".to_string()
        } else {
            rel_base.to_string_lossy().to_string()
        };
        out.push_str(&format!("scope: {scope}\n"));

        let mut queue = VecDeque::from([(base, 0usize)]);
        let mut entries_seen = 0usize;
        let mut truncated = false;

        while let Some((dir, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }

            let mut entries = fs::read_dir(&dir)
                .map_err(|err| invalid_params(format!("cannot read directory: {err}")))?
                .filter_map(Result::ok)
                .collect::<Vec<_>>();
            entries.sort_by_key(|entry| entry.file_name());

            for entry in entries {
                if entries_seen >= MAX_TREE_ENTRIES {
                    truncated = true;
                    break;
                }

                let name = entry.file_name().to_string_lossy().to_string();
                let path = entry.path();
                let is_dir = path.is_dir();
                if is_dir && IGNORED_DIRS.contains(&name.as_str()) {
                    continue;
                }

                let rel = path.strip_prefix(&self.root).unwrap_or(&path);
                out.push_str(&"  ".repeat(depth));
                out.push_str(&rel.to_string_lossy());
                if is_dir {
                    out.push('/');
                }
                out.push('\n');
                entries_seen += 1;

                if is_dir {
                    queue.push_back((path, depth + 1));
                }
            }

            if truncated {
                break;
            }
        }

        if truncated {
            out.push_str("[truncated: max entries reached]\n");
        }
        Ok(out)
    }

    fn read_file_text(&self, uri: &str) -> Result<String, ErrorData> {
        let rel = uri
            .strip_prefix(FILE_URI_PREFIX)
            .ok_or_else(|| invalid_params("unsupported resource uri"))?;
        let path = resolve_inside_root(&self.root, rel)?;
        if !path.is_file() {
            return Err(invalid_params("resource is not a file"));
        }

        let meta = fs::metadata(&path).map_err(|err| invalid_params(format!("metadata: {err}")))?;
        if meta.len() > MAX_FILE_BYTES {
            return Err(invalid_params("file exceeds 512 KiB limit"));
        }

        let bytes = fs::read(&path).map_err(|err| invalid_params(format!("read file: {err}")))?;
        if bytes.contains(&0) {
            return Err(invalid_params("binary file rejected"));
        }

        String::from_utf8(bytes).map_err(|_| invalid_params("file is not valid UTF-8 text"))
    }
}

impl ServerHandler for ReadonlyProjectServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_resources().build())
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        Ok(ListResourcesResult {
            resources: vec![Resource::new(TREE_URI, "project-tree")
                .with_description("Read-only project directory tree")
                .with_mime_type("text/plain")],
            next_cursor: None,
            meta: None,
        })
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        Ok(ListResourceTemplatesResult {
            resource_templates: vec![
                ResourceTemplate::new("ccswitch://project/tree/{path}", "project-subtree")
                    .with_description("Read a shallow project subtree")
                    .with_mime_type("text/plain"),
                ResourceTemplate::new("ccswitch://project/file/{path}", "project-file")
                    .with_description("Read a UTF-8 project file")
                    .with_mime_type("text/plain"),
            ],
            next_cursor: None,
            meta: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, ErrorData> {
        let (uri, text) = if request.uri == TREE_URI || request.uri.starts_with(TREE_URI_PREFIX) {
            let uri = request.uri;
            let text = self.tree_text(&uri)?;
            (uri, text)
        } else if request.uri.starts_with(FILE_URI_PREFIX) {
            let text = self.read_file_text(&request.uri)?;
            (request.uri, text)
        } else {
            return Err(ErrorData::resource_not_found("resource not found", None));
        };

        Ok(ReadResourceResult::new(vec![
            ResourceContents::TextResourceContents {
                uri,
                mime_type: Some("text/plain".to_string()),
                text,
                meta: None,
            },
        ]))
    }
}

fn resolve_inside_root(root: &Path, relative: &str) -> Result<PathBuf, ErrorData> {
    let rel = percent_decode(relative)?;
    let path = Path::new(&rel);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
    {
        return Err(invalid_params("path must be relative and stay under root"));
    }

    let joined = root.join(path);
    let canonical =
        fs::canonicalize(&joined).map_err(|err| invalid_params(format!("canonicalize: {err}")))?;
    if !canonical.starts_with(root) {
        return Err(invalid_params("path escapes root"));
    }
    Ok(canonical)
}

fn percent_decode(input: &str) -> Result<String, ErrorData> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err(invalid_params("invalid percent encoding"));
            }
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                .map_err(|_| invalid_params("invalid percent encoding"))?;
            let value = u8::from_str_radix(hex, 16)
                .map_err(|_| invalid_params("invalid percent encoding"))?;
            out.push(value);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| invalid_params("path is not valid UTF-8"))
}

fn invalid_params(message: impl Into<String>) -> ErrorData {
    ErrorData::invalid_params(message.into(), None)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    ReadonlyProjectServer::from_env()?
        .serve(stdio())
        .await?
        .waiting()
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn resolves_path_inside_root() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("a.txt");
        fs::write(&file, "ok").unwrap();

        let root = fs::canonicalize(dir.path()).unwrap();
        let resolved = resolve_inside_root(&root, "a.txt").unwrap();

        assert_eq!(resolved, fs::canonicalize(file).unwrap());
    }

    #[test]
    fn rejects_parent_escape() {
        let dir = tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();

        assert!(resolve_inside_root(&root, "../secret.txt").is_err());
    }

    #[test]
    fn reads_tree_and_ignores_heavy_dirs() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "pub fn x() {}").unwrap();
        fs::create_dir(dir.path().join("node_modules")).unwrap();
        fs::write(dir.path().join("node_modules/skip.js"), "skip").unwrap();

        let server = ReadonlyProjectServer {
            root: fs::canonicalize(dir.path()).unwrap(),
        };
        let tree = server.tree_text(TREE_URI).unwrap();

        assert!(tree.contains("src/"));
        assert!(tree.contains("src\\lib.rs") || tree.contains("src/lib.rs"));
        assert!(!tree.contains("node_modules"));
    }

    #[test]
    fn root_tree_is_shallow_and_subtree_can_drill_down() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src-tauri/src/proxy")).unwrap();
        fs::write(
            dir.path().join("src-tauri/src/proxy/forwarder.rs"),
            "fn main() {}",
        )
        .unwrap();

        let server = ReadonlyProjectServer {
            root: fs::canonicalize(dir.path()).unwrap(),
        };

        let root_tree = server.tree_text(TREE_URI).unwrap();
        assert!(root_tree.contains("src-tauri/"));
        assert!(!root_tree.contains("forwarder.rs"));

        let subtree = server
            .tree_text("ccswitch://project/tree/src-tauri")
            .unwrap();
        assert!(subtree.contains("src-tauri\\src/") || subtree.contains("src-tauri/src/"));
        assert!(
            subtree.contains("src-tauri\\src\\proxy/") || subtree.contains("src-tauri/src/proxy/")
        );
    }

    #[test]
    fn rejects_binary_file() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("bin.dat"), [0, 1, 2]).unwrap();
        let server = ReadonlyProjectServer {
            root: fs::canonicalize(dir.path()).unwrap(),
        };

        assert!(server
            .read_file_text("ccswitch://project/file/bin.dat")
            .is_err());
    }
}
