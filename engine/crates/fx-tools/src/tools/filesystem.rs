use super::{
    canonicalize_existing_or_parent, parse_args, to_tool_result, validate_path, ToolRegistry,
};
use crate::tool_trait::{Tool, ToolContext};
use async_trait::async_trait;
use fx_core::path::expand_tilde;
use fx_kernel::act::{JournalAction, ToolCacheability, ToolResult};
use fx_kernel::cancellation::CancellationToken;
use fx_kernel::ToolAuthoritySurface;
use fx_llm::{ToolCall, ToolDefinition};
use serde::Deserialize;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MAX_RECURSION_DEPTH: usize = 5;
pub(super) const MAX_SEARCH_MATCHES: usize = 100;

pub(super) fn register_tools(registry: &mut ToolRegistry, context: &Arc<ToolContext>) {
    registry.register(ReadFileTool::new(context));
    registry.register(WriteFileTool::new(context));
    registry.register(EditFileTool::new(context));
    registry.register(ListDirectoryTool::new(context));
    registry.register(SearchTextTool::new(context));
}

struct ReadFileTool {
    context: Arc<ToolContext>,
}

struct WriteFileTool {
    context: Arc<ToolContext>,
}

struct EditFileTool {
    context: Arc<ToolContext>,
}

struct ListDirectoryTool {
    context: Arc<ToolContext>,
}

struct SearchTextTool {
    context: Arc<ToolContext>,
}

impl ReadFileTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
        }
    }
}

impl WriteFileTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
        }
    }
}

impl EditFileTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
        }
    }
}

impl ListDirectoryTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
        }
    }
}

impl SearchTextTool {
    fn new(context: &Arc<ToolContext>) -> Self {
        Self {
            context: Arc::clone(context),
        }
    }
}

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &'static str {
        "read_file"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description:
                "Read a UTF-8 text file from disk. Supports `~` to reference the home directory."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "offset": {
                        "type": "integer",
                        "description": "Line number to start reading from (1-indexed)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of lines to return"
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        to_tool_result(
            &call.id,
            self.name(),
            self.context.handle_read_file(&call.arguments),
        )
    }

    fn cacheability(&self) -> ToolCacheability {
        ToolCacheability::Cacheable
    }

    fn action_category(&self) -> &'static str {
        "read_any"
    }

    fn authority_surface(&self, _call: &ToolCall) -> ToolAuthoritySurface {
        ToolAuthoritySurface::PathRead
    }
}

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &'static str {
        "write_file"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Write UTF-8 content to a file on disk. Supports `~` to reference the home directory."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            }),
        }
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        to_tool_result(
            &call.id,
            self.name(),
            self.context.handle_write_file(&call.arguments),
        )
    }

    fn cacheability(&self) -> ToolCacheability {
        ToolCacheability::SideEffect
    }

    fn journal_action(&self, call: &ToolCall, _result: &ToolResult) -> Option<JournalAction> {
        file_write_action(call, false)
    }

    fn action_category(&self) -> &'static str {
        "file_write"
    }

    fn authority_surface(&self, _call: &ToolCall) -> ToolAuthoritySurface {
        ToolAuthoritySurface::PathWrite
    }
}

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> &'static str {
        "edit_file"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Replace exact text in a file. The old_text must match exactly (including whitespace and newlines). Use for precise, surgical edits."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "old_text": { "type": "string" },
                    "new_text": { "type": "string" }
                },
                "required": ["path", "old_text", "new_text"]
            }),
        }
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        to_tool_result(
            &call.id,
            self.name(),
            self.context.handle_edit_file(&call.arguments),
        )
    }

    fn cacheability(&self) -> ToolCacheability {
        ToolCacheability::SideEffect
    }

    fn journal_action(&self, call: &ToolCall, _result: &ToolResult) -> Option<JournalAction> {
        file_write_action(call, false)
    }

    fn action_category(&self) -> &'static str {
        "file_write"
    }

    fn authority_surface(&self, _call: &ToolCall) -> ToolAuthoritySurface {
        ToolAuthoritySurface::PathWrite
    }
}

#[async_trait]
impl Tool for ListDirectoryTool {
    fn name(&self) -> &'static str {
        "list_directory"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description:
                "List files and directories, optionally recursively. Supports `~` to reference the home directory."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "recursive": { "type": "boolean" }
                },
                "required": ["path"]
            }),
        }
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        to_tool_result(
            &call.id,
            self.name(),
            self.context.handle_list_directory(&call.arguments),
        )
    }

    fn cacheability(&self) -> ToolCacheability {
        ToolCacheability::Cacheable
    }

    fn action_category(&self) -> &'static str {
        "read_any"
    }

    fn authority_surface(&self, _call: &ToolCall) -> ToolAuthoritySurface {
        ToolAuthoritySurface::PathRead
    }
}

#[async_trait]
impl Tool for SearchTextTool {
    fn name(&self) -> &'static str {
        "search_text"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description:
                "Search text in files and return file:line matches. Supports `~` to reference the home directory."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "path": { "type": "string" },
                    "file_glob": { "type": "string" }
                },
                "required": ["pattern"]
            }),
        }
    }

    async fn execute(&self, call: &ToolCall, _cancel: Option<&CancellationToken>) -> ToolResult {
        to_tool_result(
            &call.id,
            self.name(),
            self.context.handle_search_text(&call.arguments),
        )
    }

    fn cacheability(&self) -> ToolCacheability {
        ToolCacheability::Cacheable
    }

    fn action_category(&self) -> &'static str {
        "read_any"
    }

    fn authority_surface(&self, _call: &ToolCall) -> ToolAuthoritySurface {
        ToolAuthoritySurface::PathRead
    }
}

fn file_write_action(call: &ToolCall, created: bool) -> Option<JournalAction> {
    let path = call.arguments.get("path")?.as_str()?;
    let size_bytes = call
        .arguments
        .get("content")
        .and_then(serde_json::Value::as_str)
        .map(str::len)
        .or_else(|| {
            call.arguments
                .get("new_text")
                .and_then(serde_json::Value::as_str)
                .map(str::len)
        })
        .unwrap_or_default() as u64;
    Some(JournalAction::FileWrite {
        path: PathBuf::from(path),
        snapshot_hash: None,
        size_bytes,
        created,
    })
}

#[derive(Deserialize)]
struct ReadFileArgs {
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct WriteFileArgs {
    path: String,
    content: String,
}

#[derive(Deserialize)]
struct EditFileArgs {
    path: String,
    old_text: String,
    new_text: String,
}

#[derive(Deserialize)]
struct ListDirectoryArgs {
    path: String,
    recursive: Option<bool>,
}

#[derive(Deserialize)]
struct SearchTextArgs {
    pattern: String,
    path: Option<String>,
    file_glob: Option<String>,
}

struct EditPlan {
    updated_content: String,
    start_line: usize,
    end_line: usize,
}

impl ToolContext {
    pub(crate) fn handle_read_file(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: ReadFileArgs = parse_args(args)?;
        let path = self.resolve_read_path(&parsed.path)?;
        let content = self.read_utf8_file(&path, Some(self.config.max_read_size))?;
        render_read_output(&content, parsed.offset, parsed.limit)
    }

    pub(crate) fn handle_write_file(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: WriteFileArgs = parse_args(args)?;
        let path = self.resolve_tool_path(&parsed.path)?;
        if let Some(message) = self.apply_write_policy(&path, &parsed.content)? {
            return Ok(message);
        }
        write_text_file(&path, &parsed.content)?;
        Ok(format!(
            "wrote {} bytes to {}",
            parsed.content.len(),
            path.display()
        ))
    }

    pub(crate) fn handle_edit_file(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: EditFileArgs = parse_args(args)?;
        validate_edit_args(&parsed)?;
        let path = self.resolve_tool_path(&parsed.path)?;
        let content = self.read_utf8_file(&path, Some(self.config.max_file_size))?;
        let plan = plan_exact_edit(&path, &content, &parsed.old_text, &parsed.new_text)?;
        if let Some(message) = self.apply_write_policy(&path, &plan.updated_content)? {
            return Ok(message);
        }
        write_text_file(&path, &plan.updated_content)?;
        Ok(format!(
            "Successfully edited {} (lines {}-{})",
            path.display(),
            plan.start_line,
            plan.end_line
        ))
    }

    pub(crate) fn handle_list_directory(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: ListDirectoryArgs = parse_args(args)?;
        let path = self.resolve_read_path(&parsed.path)?;
        if parsed.recursive.unwrap_or(false) {
            return self.list_recursive(&path, 0);
        }
        self.list_flat(&path)
    }

    pub(crate) fn handle_search_text(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: SearchTextArgs = parse_args(args)?;
        let root = self.resolve_search_root(parsed.path.as_deref())?;
        let mut results = Vec::new();
        self.search_path(&root, &parsed, &mut results)?;
        Ok(results.join("\n"))
    }

    fn jailed_path(&self, requested: &str) -> Result<PathBuf, String> {
        if !self.config.jail_to_working_dir {
            return canonicalize_existing_or_parent(Path::new(requested));
        }
        validate_path(&self.working_dir, requested)
    }

    fn validated_existing_entry(&self, path: &Path) -> Result<Option<PathBuf>, String> {
        if !self.config.jail_to_working_dir {
            return Ok(Some(path.to_path_buf()));
        }
        if self.config.allow_outside_workspace_reads {
            return canonicalize_existing_or_parent(path).map(Some);
        }
        let requested = path.to_string_lossy().to_string();
        match validate_path(&self.working_dir, &requested) {
            Ok(validated) => Ok(Some(validated)),
            Err(_) => Ok(None),
        }
    }

    fn resolve_tool_path(&self, requested: &str) -> Result<PathBuf, String> {
        let expanded = expand_tilde(requested);
        let expanded_str = expanded
            .to_str()
            .ok_or_else(|| "home directory path is not valid UTF-8".to_string())?;
        self.jailed_path(expanded_str)
    }

    fn resolve_read_path(&self, requested: &str) -> Result<PathBuf, String> {
        let expanded = expand_tilde(requested);
        let expanded_str = expanded
            .to_str()
            .ok_or_else(|| "home directory path is not valid UTF-8".to_string())?;
        if !self.config.jail_to_working_dir {
            return canonicalize_existing_or_parent(Path::new(expanded_str));
        }
        if self.config.allow_outside_workspace_reads {
            return self.resolve_observation_path(expanded_str);
        }
        self.jailed_path(expanded_str)
    }

    fn resolve_observation_path(&self, requested: &str) -> Result<PathBuf, String> {
        let requested_path = Path::new(requested);
        let candidate = if requested_path.is_absolute() {
            requested_path.to_path_buf()
        } else {
            self.working_dir.join(requested_path)
        };
        canonicalize_existing_or_parent(&candidate)
    }

    fn read_utf8_file(&self, path: &Path, size_limit: Option<u64>) -> Result<String, String> {
        let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
        if size_limit.is_some_and(|limit| metadata.len() > limit) {
            return Err("file exceeds maximum allowed size".to_string());
        }
        let bytes = fs::read(path).map_err(|error| error.to_string())?;
        String::from_utf8(bytes).map_err(|_| "file appears to be binary".to_string())
    }

    fn apply_write_policy(&self, _path: &Path, content: &str) -> Result<Option<String>, String> {
        self.check_max_file_size(content.len())?;
        Ok(None)
    }

    fn check_max_file_size(&self, len: usize) -> Result<(), String> {
        if (len as u64) > self.config.max_file_size {
            return Err("content exceeds maximum allowed size".to_string());
        }
        Ok(())
    }

    fn list_flat(&self, path: &Path) -> Result<String, String> {
        let mut lines = Vec::new();
        for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let kind = entry_kind(&entry.path())?;
            lines.push(format!("[{kind}] {}", entry.file_name().to_string_lossy()));
        }
        lines.sort();
        Ok(lines.join("\n"))
    }

    fn list_recursive(&self, path: &Path, depth: usize) -> Result<String, String> {
        if depth > MAX_RECURSION_DEPTH {
            return Ok(String::new());
        }
        let mut lines = Vec::new();
        for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let entry_path = entry.path();
            if let Some(name) = entry_path.file_name().and_then(|n| n.to_str()) {
                if self.is_ignored_directory(name) && entry_path.is_dir() {
                    continue;
                }
            }
            let Some(validated) = self.validated_existing_entry(&entry_path)? else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().to_string();
            let kind = entry_kind(&entry_path)?;
            lines.push(format!("{}[{}] {}", "  ".repeat(depth), kind, name));
            if kind == "dir" {
                let nested = self.list_recursive(&validated, depth + 1)?;
                if !nested.is_empty() {
                    lines.push(nested);
                }
            }
        }
        Ok(lines.join("\n"))
    }

    fn is_ignored_directory(&self, name: &str) -> bool {
        if is_builtin_ignored_directory(name) {
            return true;
        }
        self.config.search_exclude.iter().any(|item| item == name)
    }

    fn resolve_search_root(&self, requested: Option<&str>) -> Result<PathBuf, String> {
        let default_root = self.working_dir.to_string_lossy().to_string();
        let requested = requested.unwrap_or(&default_root);
        let expanded = expand_tilde(requested);
        let expanded_str = expanded
            .to_str()
            .ok_or_else(|| "home directory path is not valid UTF-8".to_string())?;
        if !self.config.jail_to_working_dir {
            return canonicalize_existing_or_parent(Path::new(expanded_str));
        }
        if self.config.allow_outside_workspace_reads {
            return self.resolve_observation_path(expanded_str);
        }
        validate_path(&self.working_dir, expanded_str)
    }

    fn search_path(
        &self,
        root: &Path,
        args: &SearchTextArgs,
        out: &mut Vec<String>,
    ) -> Result<(), String> {
        if out.len() >= MAX_SEARCH_MATCHES {
            return Ok(());
        }
        if root.is_dir() {
            self.search_directory(root, args, out)?;
        } else {
            self.search_file(root, args, out)?;
        }
        Ok(())
    }

    fn search_directory(
        &self,
        dir: &Path,
        args: &SearchTextArgs,
        out: &mut Vec<String>,
    ) -> Result<(), String> {
        for entry in fs::read_dir(dir).map_err(|error| error.to_string())? {
            if out.len() >= MAX_SEARCH_MATCHES {
                break;
            }
            let entry_path = entry.map_err(|error| error.to_string())?.path();
            if let Some(name) = entry_path.file_name().and_then(|n| n.to_str()) {
                if self.is_ignored_directory(name) && entry_path.is_dir() {
                    continue;
                }
            }
            let Some(validated) = self.validated_existing_entry(&entry_path)? else {
                continue;
            };
            if validated.is_dir() {
                self.search_directory(&validated, args, out)?;
                continue;
            }
            self.search_file(&validated, args, out)?;
        }
        Ok(())
    }

    fn search_file(
        &self,
        file: &Path,
        args: &SearchTextArgs,
        out: &mut Vec<String>,
    ) -> Result<(), String> {
        if !matches_glob(file, args.file_glob.as_deref()) {
            return Ok(());
        }
        let metadata = fs::metadata(file).map_err(|error| error.to_string())?;
        if metadata.len() > self.config.max_read_size {
            return Ok(());
        }
        let mut bytes = Vec::new();
        let mut reader = fs::File::open(file).map_err(|error| error.to_string())?;
        reader
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        let text = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => return Ok(()),
        };
        for (index, line) in text.lines().enumerate() {
            if out.len() >= MAX_SEARCH_MATCHES {
                break;
            }
            if line.contains(&args.pattern) {
                out.push(format!("{}:{}:{}", file.display(), index + 1, line));
            }
        }
        Ok(())
    }
}

fn write_text_file(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(path, content.as_bytes()).map_err(|error| error.to_string())
}

fn validate_edit_args(args: &EditFileArgs) -> Result<(), String> {
    if args.old_text.is_empty() {
        return Err("old_text must not be empty".to_string());
    }
    if args.old_text == args.new_text {
        return Err("old_text and new_text must differ".to_string());
    }
    Ok(())
}

fn render_read_output(
    content: &str,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<String, String> {
    validate_line_window(offset, limit)?;
    if offset.is_none() && limit.is_none() {
        return Ok(content.to_string());
    }
    let lines = collect_lines(content);
    let start_line = offset.unwrap_or(1);
    if start_line > lines.len() {
        return Ok(offset_past_end_message(start_line, lines.len()));
    }
    let start_index = start_line - 1;
    let end_index = slice_end_index(start_index, limit, lines.len());
    let body = lines[start_index..end_index].concat();
    Ok(partial_read_response(
        start_line,
        end_index,
        lines.len(),
        body,
    ))
}

fn validate_line_window(offset: Option<usize>, limit: Option<usize>) -> Result<(), String> {
    if offset == Some(0) {
        return Err("offset must be at least 1".to_string());
    }
    if limit == Some(0) {
        return Err("limit must be at least 1".to_string());
    }
    Ok(())
}

fn collect_lines(content: &str) -> Vec<&str> {
    if content.is_empty() {
        return Vec::new();
    }
    content.split_inclusive('\n').collect()
}

fn offset_past_end_message(start_line: usize, total_lines: usize) -> String {
    format!("(no lines returned; offset {start_line} is past end of file with {total_lines} lines)")
}

fn slice_end_index(start_index: usize, limit: Option<usize>, total_lines: usize) -> usize {
    match limit {
        Some(limit) => (start_index + limit).min(total_lines),
        None => total_lines,
    }
}

fn partial_read_response(
    start_line: usize,
    end_index: usize,
    total_lines: usize,
    body: String,
) -> String {
    let header = format!("[Lines {start_line}-{end_index} of {total_lines}]");
    if body.is_empty() {
        header
    } else {
        format!("{header}\n{body}")
    }
}

fn plan_exact_edit(
    path: &Path,
    content: &str,
    old_text: &str,
    new_text: &str,
) -> Result<EditPlan, String> {
    let matches = count_exact_matches(content, old_text);
    if matches == 0 {
        return Err(format!(
            "Could not find the exact text in {}. The old_text must match exactly including all whitespace and newlines.",
            path.display()
        ));
    }
    if matches > 1 {
        return Err(format!(
            "Found {matches} matches for old_text in {}. Please provide more context to uniquely identify the target.",
            path.display()
        ));
    }
    let start = content.find(old_text).ok_or_else(|| {
        format!(
            "Could not find the exact text in {}. The old_text must match exactly including all whitespace and newlines.",
            path.display()
        )
    })?;
    let (start_line, end_line) = line_span(content, start, old_text);
    Ok(EditPlan {
        updated_content: replace_exact_range(content, start, old_text, new_text),
        start_line,
        end_line,
    })
}

fn count_exact_matches(content: &str, needle: &str) -> usize {
    let haystack = content.as_bytes();
    let needle = needle.as_bytes();
    if needle.is_empty() || needle.len() > haystack.len() {
        return 0;
    }
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

fn line_span(content: &str, start: usize, old_text: &str) -> (usize, usize) {
    let start_line = content[..start]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1;
    let line_count = old_text.bytes().filter(|byte| *byte == b'\n').count() + 1;
    (start_line, start_line + line_count - 1)
}

fn replace_exact_range(content: &str, start: usize, old_text: &str, new_text: &str) -> String {
    let mut updated = String::with_capacity(content.len() - old_text.len() + new_text.len());
    updated.push_str(&content[..start]);
    updated.push_str(new_text);
    updated.push_str(&content[start + old_text.len()..]);
    updated
}

fn entry_kind(path: &Path) -> Result<&'static str, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    let kind = if metadata.file_type().is_dir() {
        "dir"
    } else if metadata.file_type().is_symlink() {
        "symlink"
    } else {
        "file"
    };
    Ok(kind)
}

fn matches_glob(path: &Path, file_glob: Option<&str>) -> bool {
    let Some(pattern) = file_glob else {
        return true;
    };
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    simple_glob_match(name, pattern)
}

pub(super) fn is_builtin_ignored_directory(name: &str) -> bool {
    matches!(
        name,
        "target"
            | ".git"
            | "node_modules"
            | ".build"
            | "build"
            | ".gradle"
            | "__pycache__"
            | ".mypy_cache"
            | ".pytest_cache"
            | "dist"
            | ".next"
            | ".turbo"
    )
}

fn simple_glob_match(name: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return name == pattern;
    }
    let parts = pattern.split('*').collect::<Vec<_>>();
    if parts.len() == 2 {
        return name.starts_with(parts[0]) && name.ends_with(parts[1]);
    }
    name.contains(&pattern.replace('*', ""))
}
