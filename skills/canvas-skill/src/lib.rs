use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::fmt;

const INFO_LEVEL: u32 = 2;
const ERROR_LEVEL: u32 = 4;
const MAX_HOST_STRING_LEN: usize = 65_536;
const BAR_WIDTH: usize = 16;
const PIE_BAR_WIDTH: usize = 50;

#[link(wasm_import_module = "host_api_v1")]
extern "C" {
    #[link_name = "log"]
    fn host_log(level: u32, msg_ptr: *const u8, msg_len: u32);
    #[link_name = "get_input"]
    fn host_get_input() -> u32;
    #[link_name = "set_output"]
    fn host_set_output(text_ptr: *const u8, text_len: u32);
}

#[derive(Debug, Deserialize)]
#[serde(tag = "tool")]
enum Input {
    #[serde(rename = "render_table")]
    Table(TableInput),
    #[serde(rename = "render_chart")]
    Chart(ChartInput),
    #[serde(rename = "render_document")]
    Document(DocumentInput),
}

#[derive(Debug, Deserialize)]
struct TableInput {
    headers: String,
    rows: String,
    title: Option<String>,
    alignment: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChartInput {
    chart_type: String,
    data: String,
    title: Option<String>,
    x_label: Option<String>,
    y_label: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DocumentInput {
    sections: String,
    title: Option<String>,
    format: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawChartData {
    labels: Option<Vec<String>>,
    values: Option<Vec<f64>>,
}

#[derive(Debug, Clone, PartialEq)]
struct ChartData {
    labels: Vec<String>,
    values: Vec<f64>,
}

#[derive(Debug)]
struct TableResult {
    title: Option<String>,
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    alignment: Vec<String>,
    column_widths: Vec<usize>,
    markdown: String,
    message: String,
}

#[derive(Debug)]
struct ChartResult {
    chart_type: ChartType,
    title: Option<String>,
    x_label: Option<String>,
    y_label: Option<String>,
    data: ChartData,
    text_fallback: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct RawSection {
    heading: Option<String>,
    content: String,
    #[serde(rename = "type")]
    kind: Option<String>,
    language: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Section {
    heading: Option<String>,
    content: String,
    kind: SectionType,
    language: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SectionType {
    Text,
    Code,
    Quote,
    List,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChartType {
    Bar,
    Line,
    Pie,
    Scatter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Canvas,
    Markdown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CanvasError {
    InvalidInput(String),
}

impl fmt::Display for CanvasError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message) => formatter.write_str(message),
        }
    }
}

/// # Safety
/// `ptr` must be 0 or point to a NUL-terminated string in valid WASM linear memory.
unsafe fn read_host_string(ptr: u32) -> Option<String> {
    if ptr == 0 {
        return None;
    }

    let slice = core::slice::from_raw_parts(ptr as *const u8, MAX_HOST_STRING_LEN);
    let len = slice
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(MAX_HOST_STRING_LEN);
    Some(String::from_utf8_lossy(&slice[..len]).into_owned())
}

fn log(level: u32, message: &str) {
    unsafe {
        host_log(level, message.as_ptr(), message.len() as u32);
    }
}

fn get_input() -> String {
    unsafe { read_host_string(host_get_input()).unwrap_or_default() }
}

fn set_output(text: &str) {
    unsafe {
        host_set_output(text.as_ptr(), text.len() as u32);
    }
}

fn execute(raw_input: &str) -> Result<String, CanvasError> {
    match parse_input(raw_input)? {
        Input::Table(input) => render_table(input),
        Input::Chart(input) => render_chart(input),
        Input::Document(input) => render_document(input),
    }
}

fn parse_input(raw_input: &str) -> Result<Input, CanvasError> {
    serde_json::from_str(raw_input).map_err(|error| {
        CanvasError::InvalidInput(format!(
            "Invalid input: {error}. Expected 'tool': 'render_table', 'render_chart', or 'render_document'."
        ))
    })
}

fn render_table(input: TableInput) -> Result<String, CanvasError> {
    let headers = parse_headers(&input.headers)?;
    let rows = parse_rows(&input.rows)?;
    validate_row_lengths(headers.len(), &rows)?;
    let alignment = parse_alignment(input.alignment.as_deref(), headers.len())?;
    let title = normalized_text(input.title.as_deref());
    let result = TableResult {
        message: table_message(title.as_deref(), rows.len(), headers.len()),
        markdown: markdown_table(&headers, &rows),
        column_widths: column_widths(&headers, &rows),
        title,
        headers,
        rows,
        alignment,
    };

    Ok(table_output(result))
}

fn parse_headers(headers: &str) -> Result<Vec<String>, CanvasError> {
    // Headers are currently parsed from a comma-delimited string, so values containing
    // commas will be split incorrectly. A future version should accept a JSON array.
    let parsed: Vec<String> = headers
        .split(',')
        .map(|value| value.trim().to_string())
        .collect();
    if parsed.is_empty() || parsed.iter().all(|value| value.is_empty()) {
        return Err(CanvasError::InvalidInput(
            "Table headers must not be empty.".to_string(),
        ));
    }
    if parsed.iter().any(|value| value.is_empty()) {
        return Err(CanvasError::InvalidInput(
            "Table headers must not contain empty values.".to_string(),
        ));
    }
    Ok(parsed)
}

fn parse_rows(rows: &str) -> Result<Vec<Vec<String>>, CanvasError> {
    let parsed: Vec<Vec<Value>> = serde_json::from_str(rows).map_err(|error| {
        CanvasError::InvalidInput(format!("Rows must be a JSON array of arrays: {error}"))
    })?;

    Ok(parsed
        .into_iter()
        .map(|row| row.into_iter().map(cell_text).collect())
        .collect())
}

fn cell_text(value: Value) -> String {
    match value {
        Value::String(text) => text,
        Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

fn validate_row_lengths(columns: usize, rows: &[Vec<String>]) -> Result<(), CanvasError> {
    for (index, row) in rows.iter().enumerate() {
        if row.len() != columns {
            return Err(CanvasError::InvalidInput(format!(
                "Row {} has {} columns but expected {}.",
                index + 1,
                row.len(),
                columns
            )));
        }
    }
    Ok(())
}

fn parse_alignment(value: Option<&str>, columns: usize) -> Result<Vec<String>, CanvasError> {
    let Some(value) = value else {
        return Ok(vec!["left".to_string(); columns]);
    };

    let alignments: Vec<String> = value
        .split(',')
        .map(|item| item.trim().to_lowercase())
        .collect();
    if alignments.len() != columns {
        return Err(CanvasError::InvalidInput(format!(
            "Alignment count {} does not match header count {}.",
            alignments.len(),
            columns
        )));
    }
    validate_alignments(&alignments)?;
    Ok(alignments)
}

fn validate_alignments(alignments: &[String]) -> Result<(), CanvasError> {
    for alignment in alignments {
        if !matches!(alignment.as_str(), "left" | "right" | "center") {
            return Err(CanvasError::InvalidInput(format!(
                "Invalid alignment '{alignment}'. Use 'left', 'right', or 'center'."
            )));
        }
    }
    Ok(())
}

fn column_widths(headers: &[String], rows: &[Vec<String>]) -> Vec<usize> {
    let mut widths: Vec<usize> = headers
        .iter()
        .map(|header| header.chars().count())
        .collect();
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(cell.chars().count());
        }
    }
    widths
}

fn markdown_table(headers: &[String], rows: &[Vec<String>]) -> String {
    let header_line = table_line(headers.iter().map(String::as_str).collect());
    let separator = table_line(vec!["---"; headers.len()]);
    let row_lines: Vec<String> = rows
        .iter()
        .map(|row| table_line(row.iter().map(String::as_str).collect()))
        .collect();

    let mut lines = vec![header_line, separator];
    lines.extend(row_lines);
    lines.join("\n")
}

fn table_line(cells: Vec<&str>) -> String {
    let escaped: Vec<String> = cells.into_iter().map(escape_markdown_cell).collect();
    format!("| {} |", escaped.join(" | "))
}

fn escape_markdown_cell(cell: &str) -> String {
    cell.replace('|', "\\|")
}

fn table_message(title: Option<&str>, rows: usize, columns: usize) -> String {
    format!(
        "📊 Table: {} ({} {} × {} columns)",
        title.unwrap_or("Untitled"),
        rows,
        pluralize(rows, "row"),
        columns
    )
}

fn table_output(result: TableResult) -> String {
    let mut canvas = Map::new();
    canvas.insert("kind".to_string(), json!("table"));
    canvas.insert("headers".to_string(), json!(result.headers));
    canvas.insert("rows".to_string(), json!(result.rows));
    canvas.insert("alignment".to_string(), json!(result.alignment));
    canvas.insert("column_widths".to_string(), json!(result.column_widths));

    let mut output = Map::new();
    output.insert("status".to_string(), json!("success"));
    output.insert("type".to_string(), json!("table"));
    if let Some(title) = result.title {
        output.insert("title".to_string(), json!(title));
    }
    output.insert("canvas".to_string(), Value::Object(canvas));
    output.insert("markdown".to_string(), json!(result.markdown));
    output.insert("message".to_string(), json!(result.message));
    Value::Object(output).to_string()
}

fn render_chart(input: ChartInput) -> Result<String, CanvasError> {
    let chart_type = ChartType::parse(&input.chart_type)?;
    let data = parse_chart_data(&input.data)?;
    let title = normalized_text(input.title.as_deref());
    let result = ChartResult {
        message: chart_message(chart_type, title.as_deref(), data.values.len()),
        text_fallback: chart_fallback(chart_type, title.as_deref(), &data)?,
        chart_type,
        title,
        x_label: normalized_text(input.x_label.as_deref()),
        y_label: normalized_text(input.y_label.as_deref()),
        data,
    };

    Ok(chart_output(result))
}

fn parse_chart_data(data: &str) -> Result<ChartData, CanvasError> {
    let parsed: RawChartData = serde_json::from_str(data).map_err(|error| {
        CanvasError::InvalidInput(format!(
            "Chart data must be a JSON object with 'labels' and 'values': {error}"
        ))
    })?;
    let labels = parsed.labels.ok_or_else(missing_labels_error)?;
    let values = parsed.values.ok_or_else(missing_values_error)?;
    validate_chart_lengths(labels.len(), values.len())?;
    Ok(ChartData { labels, values })
}

fn missing_labels_error() -> CanvasError {
    CanvasError::InvalidInput("Chart data must include a 'labels' array.".to_string())
}

fn missing_values_error() -> CanvasError {
    CanvasError::InvalidInput("Chart data must include a 'values' array.".to_string())
}

fn validate_chart_lengths(labels: usize, values: usize) -> Result<(), CanvasError> {
    if labels == 0 || values == 0 {
        return Err(CanvasError::InvalidInput(
            "Chart data must include at least one label and one value.".to_string(),
        ));
    }
    if labels != values {
        return Err(CanvasError::InvalidInput(format!(
            "Chart labels count {} does not match values count {}.",
            labels, values
        )));
    }
    Ok(())
}

impl ChartType {
    fn parse(value: &str) -> Result<Self, CanvasError> {
        match value.trim().to_lowercase().as_str() {
            "bar" => Ok(Self::Bar),
            "line" => Ok(Self::Line),
            "pie" => Ok(Self::Pie),
            "scatter" => Ok(Self::Scatter),
            other => Err(CanvasError::InvalidInput(format!(
                "Invalid chart_type '{other}'. Use 'bar', 'line', 'pie', or 'scatter'."
            ))),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Bar => "bar",
            Self::Line => "line",
            Self::Pie => "pie",
            Self::Scatter => "scatter",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Bar => "Bar",
            Self::Line => "Line",
            Self::Pie => "Pie",
            Self::Scatter => "Scatter",
        }
    }
}

fn chart_fallback(
    chart_type: ChartType,
    title: Option<&str>,
    data: &ChartData,
) -> Result<String, CanvasError> {
    match chart_type {
        ChartType::Bar => {
            validate_bar_chart_values(data)?;
            Ok(bar_chart_fallback(title, data))
        }
        ChartType::Line => Ok(line_chart_fallback(title, data)),
        ChartType::Pie => pie_chart_fallback(title, data),
        ChartType::Scatter => Ok(scatter_chart_fallback(title, data)),
    }
}

fn validate_bar_chart_values(data: &ChartData) -> Result<(), CanvasError> {
    if data.values.iter().any(|value| *value < 0.0) {
        return Err(CanvasError::InvalidInput(
            "Bar chart values must be zero or greater; negative values are not supported."
                .to_string(),
        ));
    }
    Ok(())
}

fn bar_chart_fallback(title: Option<&str>, data: &ChartData) -> String {
    let max_value = data.values.iter().copied().fold(0.0_f64, f64::max);
    let mut lines = vec![title.unwrap_or("Bar Chart").to_string()];
    for (label, value) in data.labels.iter().zip(&data.values) {
        let bar = scaled_bar(*value, max_value, BAR_WIDTH);
        lines.push(format!("  {label}: {bar} {}", number_text(*value)));
    }
    lines.join("\n")
}

fn line_chart_fallback(title: Option<&str>, data: &ChartData) -> String {
    let mut lines = vec![title.unwrap_or("Line Chart").to_string()];
    for index in 0..data.labels.len() {
        lines.push(line_chart_entry(index, data));
    }
    lines.join("\n")
}

fn line_chart_entry(index: usize, data: &ChartData) -> String {
    let label = &data.labels[index];
    let value = number_text(data.values[index]);
    match index {
        0 => format!("  {label}: {value}"),
        _ => format!(
            "  {label}: {value} {}",
            trend_arrow(data.values[index - 1], data.values[index])
        ),
    }
}

fn trend_arrow(previous: f64, current: f64) -> &'static str {
    if current > previous {
        "↗"
    } else if current < previous {
        "↘"
    } else {
        "→"
    }
}

fn pie_chart_fallback(title: Option<&str>, data: &ChartData) -> Result<String, CanvasError> {
    let total = data.values.iter().sum::<f64>();
    if total <= 0.0 {
        return Err(CanvasError::InvalidInput(
            "Pie chart values must sum to more than zero.".to_string(),
        ));
    }

    let mut lines = vec![title.unwrap_or("Pie Chart").to_string()];
    for (label, value) in data.labels.iter().zip(&data.values) {
        let percentage = (value / total) * 100.0;
        let bar = scaled_bar(percentage, 100.0, PIE_BAR_WIDTH);
        lines.push(format!("  {label}: {:>5.1}% {bar}", percentage));
    }
    Ok(lines.join("\n"))
}

fn scatter_chart_fallback(title: Option<&str>, data: &ChartData) -> String {
    let mut lines = vec![title.unwrap_or("Scatter Plot").to_string()];
    for (label, value) in data.labels.iter().zip(&data.values) {
        lines.push(format!("  ({label}, {})", number_text(*value)));
    }
    lines.join("\n")
}

fn scaled_bar(value: f64, max_value: f64, width: usize) -> String {
    let filled = scaled_bar_width(value, max_value, width);
    format!(
        "{}{}",
        "█".repeat(filled),
        "░".repeat(width.saturating_sub(filled))
    )
}

fn scaled_bar_width(value: f64, max_value: f64, width: usize) -> usize {
    if max_value <= 0.0 || value <= 0.0 {
        return 0;
    }
    let ratio = (value / max_value).clamp(0.0, 1.0);
    (ratio * width as f64).round() as usize
}

fn number_text(value: f64) -> String {
    // These values come straight from parsed JSON, not from accumulated floating-point
    // arithmetic, so checking for an exactly empty fractional part is safe here.
    if value.fract() == 0.0 {
        return format!("{}", value as i64);
    }
    format!("{value}")
}

fn chart_message(chart_type: ChartType, title: Option<&str>, points: usize) -> String {
    format!(
        "📈 {} chart: {} ({} data {})",
        chart_type.label(),
        title.unwrap_or("Untitled"),
        points,
        pluralize(points, "point")
    )
}

fn chart_output(result: ChartResult) -> String {
    let mut canvas = Map::new();
    canvas.insert("kind".to_string(), json!(result.chart_type.as_str()));
    if let Some(title) = &result.title {
        canvas.insert("title".to_string(), json!(title));
    }
    canvas.insert("labels".to_string(), json!(result.data.labels));
    canvas.insert(
        "datasets".to_string(),
        json!([{ "values": result.data.values }]),
    );
    if let Some(x_label) = result.x_label {
        canvas.insert("x_label".to_string(), json!(x_label));
    }
    if let Some(y_label) = result.y_label {
        canvas.insert("y_label".to_string(), json!(y_label));
    }

    let mut output = Map::new();
    output.insert("status".to_string(), json!("success"));
    output.insert("type".to_string(), json!("chart"));
    output.insert("canvas".to_string(), Value::Object(canvas));
    output.insert("text_fallback".to_string(), json!(result.text_fallback));
    output.insert("message".to_string(), json!(result.message));
    Value::Object(output).to_string()
}

fn render_document(input: DocumentInput) -> Result<String, CanvasError> {
    let sections = parse_sections(&input.sections)?;
    let title = normalized_text(input.title.as_deref());
    let markdown = markdown_document(title.as_deref(), &sections);

    match OutputFormat::parse(input.format.as_deref())? {
        OutputFormat::Markdown => Ok(document_markdown_output(title, markdown, sections.len())),
        OutputFormat::Canvas => Ok(document_output(title, sections, markdown)),
    }
}

fn parse_sections(sections: &str) -> Result<Vec<Section>, CanvasError> {
    let parsed: Vec<RawSection> = serde_json::from_str(sections).map_err(|error| {
        CanvasError::InvalidInput(format!(
            "Sections must be a JSON array of section objects: {error}"
        ))
    })?;
    if parsed.is_empty() {
        return Err(CanvasError::InvalidInput(
            "Document sections must not be empty.".to_string(),
        ));
    }

    parsed.into_iter().map(parse_section).collect()
}

fn parse_section(section: RawSection) -> Result<Section, CanvasError> {
    let content = section.content.trim().to_string();
    if content.is_empty() {
        return Err(CanvasError::InvalidInput(
            "Document sections must include content.".to_string(),
        ));
    }

    Ok(Section {
        heading: normalized_text(section.heading.as_deref()),
        content,
        kind: SectionType::parse(section.kind.as_deref())?,
        language: normalized_text(section.language.as_deref()),
    })
}

impl SectionType {
    fn parse(value: Option<&str>) -> Result<Self, CanvasError> {
        match value.map(str::trim).filter(|text| !text.is_empty()) {
            None => Ok(Self::Text),
            Some("text") => Ok(Self::Text),
            Some("code") => Ok(Self::Code),
            Some("quote") => Ok(Self::Quote),
            Some("list") => Ok(Self::List),
            Some(other) => Err(CanvasError::InvalidInput(format!(
                "Invalid section type '{other}'. Use 'text', 'code', 'quote', or 'list'."
            ))),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Code => "code",
            Self::Quote => "quote",
            Self::List => "list",
        }
    }
}

impl OutputFormat {
    fn parse(value: Option<&str>) -> Result<Self, CanvasError> {
        match value.map(str::trim).filter(|text| !text.is_empty()) {
            None => Ok(Self::Canvas),
            Some("canvas") => Ok(Self::Canvas),
            Some("markdown") => Ok(Self::Markdown),
            Some(other) => Err(CanvasError::InvalidInput(format!(
                "Invalid format '{other}'. Use 'canvas' or 'markdown'."
            ))),
        }
    }
}

fn markdown_document(title: Option<&str>, sections: &[Section]) -> String {
    let mut blocks = Vec::new();
    if let Some(title) = title {
        blocks.push(format!("# {title}"));
    }
    for section in sections {
        blocks.push(markdown_section(section));
    }
    blocks.join("\n\n")
}

fn markdown_section(section: &Section) -> String {
    let mut parts = Vec::new();
    if let Some(heading) = &section.heading {
        parts.push(format!("## {heading}"));
    }
    parts.push(markdown_section_body(section));
    parts.join("\n\n")
}

fn markdown_section_body(section: &Section) -> String {
    match section.kind {
        SectionType::Text => section.content.clone(),
        SectionType::Code => markdown_code_block(section),
        SectionType::Quote => quote_block(&section.content),
        SectionType::List => list_block(&section.content),
    }
}

fn markdown_code_block(section: &Section) -> String {
    match &section.language {
        Some(language) => format!("```{language}\n{}\n```", section.content),
        None => format!("```\n{}\n```", section.content),
    }
}

fn quote_block(content: &str) -> String {
    content
        .lines()
        .map(|line| format!("> {line}"))
        .collect::<Vec<String>>()
        .join("\n")
}

fn list_block(content: &str) -> String {
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| format!("- {line}"))
        .collect::<Vec<String>>()
        .join("\n")
}

fn document_output(title: Option<String>, sections: Vec<Section>, markdown: String) -> String {
    let canvas_sections: Vec<Value> = sections.into_iter().map(section_value).collect();
    let message = document_message(title.as_deref(), canvas_sections.len());

    let mut canvas = Map::new();
    canvas.insert("kind".to_string(), json!("document"));
    canvas.insert("sections".to_string(), json!(canvas_sections));

    let mut output = document_output_map(title, markdown, message);
    output.insert("canvas".to_string(), Value::Object(canvas));
    Value::Object(output).to_string()
}

fn document_markdown_output(title: Option<String>, markdown: String, sections: usize) -> String {
    let output = document_output_map(
        title.clone(),
        markdown,
        document_message(title.as_deref(), sections),
    );
    Value::Object(output).to_string()
}

fn document_output_map(
    title: Option<String>,
    markdown: String,
    message: String,
) -> Map<String, Value> {
    let mut output = Map::new();
    output.insert("status".to_string(), json!("success"));
    output.insert("type".to_string(), json!("document"));
    if let Some(title) = title {
        output.insert("title".to_string(), json!(title));
    }
    output.insert("markdown".to_string(), json!(markdown));
    output.insert("message".to_string(), json!(message));
    output
}

fn section_value(section: Section) -> Value {
    let mut value = Map::new();
    if let Some(heading) = section.heading {
        value.insert("heading".to_string(), json!(heading));
    }
    value.insert("content".to_string(), json!(section.content));
    value.insert("type".to_string(), json!(section.kind.as_str()));
    if let Some(language) = section.language {
        value.insert("language".to_string(), json!(language));
    }
    Value::Object(value)
}

fn document_message(title: Option<&str>, sections: usize) -> String {
    format!(
        "📄 Document: {} ({} {})",
        title.unwrap_or("Untitled"),
        sections,
        pluralize(sections, "section")
    )
}

fn normalized_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn pluralize(count: usize, singular: &str) -> String {
    if count == 1 {
        singular.to_string()
    } else {
        format!("{singular}s")
    }
}

fn error_output(error: &CanvasError) -> String {
    json!({ "error": error.to_string() }).to_string()
}

#[no_mangle]
pub extern "C" fn run() {
    log(INFO_LEVEL, "Canvas skill starting");
    let input = get_input();

    match execute(&input) {
        Ok(output) => set_output(&output),
        Err(error) => {
            log(ERROR_LEVEL, &error.to_string());
            set_output(&error_output(&error));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_routing_handles_render_table() {
        let output = execute(
            r#"{
                "tool":"render_table",
                "headers":"Name,Revenue",
                "rows":"[[\"Widget A\",\"$1.2M\"]]"
            }"#,
        )
        .expect("table should render");
        let parsed: Value = serde_json::from_str(&output).expect("json output");

        assert_eq!(parsed["type"], "table");
        assert_eq!(parsed["canvas"]["kind"], "table");
    }

    #[test]
    fn tool_routing_handles_render_chart() {
        let output = execute(
            r#"{
                "tool":"render_chart",
                "chart_type":"bar",
                "data":"{\"labels\":[\"Jan\"],\"values\":[100]}"
            }"#,
        )
        .expect("chart should render");
        let parsed: Value = serde_json::from_str(&output).expect("json output");

        assert_eq!(parsed["type"], "chart");
        assert_eq!(parsed["canvas"]["kind"], "bar");
    }

    #[test]
    fn tool_routing_handles_render_document() {
        let output = execute(
            r#"{
                "tool":"render_document",
                "sections":"[{\"heading\":\"Overview\",\"content\":\"Hello\"}]"
            }"#,
        )
        .expect("document should render");
        let parsed: Value = serde_json::from_str(&output).expect("json output");

        assert_eq!(parsed["type"], "document");
        assert_eq!(parsed["canvas"]["kind"], "document");
    }

    #[test]
    fn tool_routing_rejects_unknown_tool() {
        let error = execute(r#"{"tool":"unknown"}"#).expect_err("tool should fail");
        assert_eq!(
            error.to_string(),
            "Invalid input: unknown variant `unknown`, expected one of `render_table`, `render_chart`, `render_document` at line 1 column 17. Expected 'tool': 'render_table', 'render_chart', or 'render_document'."
        );
    }

    #[test]
    fn render_table_parses_headers_and_calculates_widths() {
        let output = render_table(TableInput {
            headers: "Name, Revenue, Growth".to_string(),
            rows: r#"[["Widget A","$1.2M","+12%"]]"#.to_string(),
            title: Some("Sales Report".to_string()),
            alignment: None,
        })
        .expect("table should render");
        let parsed: Value = serde_json::from_str(&output).expect("json output");

        assert_eq!(parsed["title"], "Sales Report");
        assert_eq!(
            parsed["canvas"]["headers"],
            json!(["Name", "Revenue", "Growth"])
        );
        assert_eq!(parsed["canvas"]["column_widths"], json!([8, 7, 6]));
        assert_eq!(
            parsed["markdown"],
            "| Name | Revenue | Growth |\n| --- | --- | --- |\n| Widget A | $1.2M | +12% |"
        );
    }

    #[test]
    fn render_table_escapes_pipe_characters_in_markdown_cells() {
        let output = render_table(TableInput {
            headers: "Name, Notes".to_string(),
            rows: r#"[["Widget A","A|B"]]"#.to_string(),
            title: None,
            alignment: None,
        })
        .expect("table should render");
        let parsed: Value = serde_json::from_str(&output).expect("json output");

        assert_eq!(
            parsed["markdown"],
            "| Name | Notes |\n| --- | --- |\n| Widget A | A\\|B |"
        );
    }

    #[test]
    fn render_table_stringifies_non_string_cells() {
        let output = render_table(TableInput {
            headers: "Count, Enabled, Missing".to_string(),
            rows: r#"[[42,true,null]]"#.to_string(),
            title: None,
            alignment: None,
        })
        .expect("table should render");
        let parsed: Value = serde_json::from_str(&output).expect("json output");

        assert_eq!(parsed["canvas"]["rows"], json!([["42", "true", "null"]]));
        assert_eq!(
            parsed["markdown"],
            "| Count | Enabled | Missing |\n| --- | --- | --- |\n| 42 | true | null |"
        );
    }

    #[test]
    fn render_table_uses_alignment_when_provided() {
        let output = render_table(TableInput {
            headers: "Name, Revenue, Growth".to_string(),
            rows: r#"[["Widget A","$1.2M","+12%"]]"#.to_string(),
            title: None,
            alignment: Some("left, right, center".to_string()),
        })
        .expect("table should render");
        let parsed: Value = serde_json::from_str(&output).expect("json output");

        assert_eq!(
            parsed["canvas"]["alignment"],
            json!(["left", "right", "center"])
        );
    }

    #[test]
    fn render_table_rejects_mismatched_columns() {
        let error = render_table(TableInput {
            headers: "Name, Revenue".to_string(),
            rows: r#"[["Widget A"]]"#.to_string(),
            title: None,
            alignment: None,
        })
        .expect_err("table should fail");

        assert_eq!(error.to_string(), "Row 1 has 1 columns but expected 2.");
    }

    #[test]
    fn render_table_rejects_invalid_alignment() {
        let error = render_table(TableInput {
            headers: "Name, Revenue".to_string(),
            rows: r#"[["Widget A","$1.2M"]]"#.to_string(),
            title: None,
            alignment: Some("left, diagonal".to_string()),
        })
        .expect_err("alignment should fail");

        assert_eq!(
            error.to_string(),
            "Invalid alignment 'diagonal'. Use 'left', 'right', or 'center'."
        );
    }

    #[test]
    fn render_table_rejects_empty_headers() {
        let error = parse_headers("   ").expect_err("headers should fail");
        assert_eq!(error.to_string(), "Table headers must not be empty.");
    }

    #[test]
    fn render_chart_supports_bar_output() {
        let output = render_chart(ChartInput {
            chart_type: "bar".to_string(),
            data: r#"{"labels":["Jan","Feb","Mar"],"values":[100,150,200]}"#.to_string(),
            title: Some("Monthly Revenue".to_string()),
            x_label: Some("Month".to_string()),
            y_label: Some("Revenue ($)".to_string()),
        })
        .expect("chart should render");
        let parsed: Value = serde_json::from_str(&output).expect("json output");

        assert_eq!(parsed["canvas"]["kind"], "bar");
        assert_eq!(parsed["canvas"]["x_label"], "Month");
        assert_eq!(parsed["canvas"]["y_label"], "Revenue ($)");
        assert_eq!(
            parsed["text_fallback"],
            "Monthly Revenue\n  Jan: ████████░░░░░░░░ 100\n  Feb: ████████████░░░░ 150\n  Mar: ████████████████ 200"
        );
    }

    #[test]
    fn render_chart_rejects_negative_bar_values() {
        let error = render_chart(ChartInput {
            chart_type: "bar".to_string(),
            data: r#"{"labels":["Jan","Feb","Mar"],"values":[100,-50,200]}"#.to_string(),
            title: None,
            x_label: None,
            y_label: None,
        })
        .expect_err("negative values should fail");

        assert_eq!(
            error.to_string(),
            "Bar chart values must be zero or greater; negative values are not supported."
        );
    }

    #[test]
    fn render_chart_supports_line_output() {
        let output = render_chart(ChartInput {
            chart_type: "line".to_string(),
            data: r#"{"labels":["Jan","Feb","Mar","Apr"],"values":[100,150,120,120]}"#.to_string(),
            title: Some("Monthly Trend".to_string()),
            x_label: None,
            y_label: None,
        })
        .expect("chart should render");
        let parsed: Value = serde_json::from_str(&output).expect("json output");

        assert_eq!(parsed["canvas"]["kind"], "line");
        assert_eq!(
            parsed["text_fallback"],
            "Monthly Trend\n  Jan: 100\n  Feb: 150 ↗\n  Mar: 120 ↘\n  Apr: 120 →"
        );
    }

    #[test]
    fn render_chart_supports_pie_output() {
        let output = render_chart(ChartInput {
            chart_type: "pie".to_string(),
            data: r#"{"labels":["Chrome","Firefox","Safari"],"values":[65,20,15]}"#.to_string(),
            title: Some("Market Share".to_string()),
            x_label: None,
            y_label: None,
        })
        .expect("chart should render");
        let parsed: Value = serde_json::from_str(&output).expect("json output");
        let fallback = parsed["text_fallback"].as_str().expect("text fallback");

        assert_eq!(parsed["canvas"]["kind"], "pie");
        assert!(fallback.contains("Chrome:  65.0%"));
        assert!(fallback.contains("Firefox:  20.0%"));
        assert!(fallback.contains("Safari:  15.0%"));
    }

    #[test]
    fn render_chart_supports_scatter_output() {
        let output = render_chart(ChartInput {
            chart_type: "scatter".to_string(),
            data: r#"{"labels":["1","2","3"],"values":[10,20,30]}"#.to_string(),
            title: Some("Points".to_string()),
            x_label: None,
            y_label: None,
        })
        .expect("chart should render");
        let parsed: Value = serde_json::from_str(&output).expect("json output");

        assert_eq!(parsed["canvas"]["kind"], "scatter");
        assert_eq!(
            parsed["text_fallback"],
            "Points\n  (1, 10)\n  (2, 20)\n  (3, 30)"
        );
    }

    #[test]
    fn render_chart_rejects_mismatched_lengths() {
        let error = render_chart(ChartInput {
            chart_type: "bar".to_string(),
            data: r#"{"labels":["Jan"],"values":[100,200]}"#.to_string(),
            title: None,
            x_label: None,
            y_label: None,
        })
        .expect_err("chart should fail");

        assert_eq!(
            error.to_string(),
            "Chart labels count 1 does not match values count 2."
        );
    }

    #[test]
    fn render_chart_rejects_missing_labels() {
        let error = parse_chart_data(r#"{"values":[1,2,3]}"#).expect_err("labels required");
        assert_eq!(
            error.to_string(),
            "Chart data must include a 'labels' array."
        );
    }

    #[test]
    fn render_chart_rejects_missing_values() {
        let error = parse_chart_data(r#"{"labels":["Jan"]}"#).expect_err("values required");
        assert_eq!(
            error.to_string(),
            "Chart data must include a 'values' array."
        );
    }

    #[test]
    fn render_chart_rejects_unknown_chart_type() {
        let error = ChartType::parse("radar").expect_err("chart type should fail");
        assert_eq!(
            error.to_string(),
            "Invalid chart_type 'radar'. Use 'bar', 'line', 'pie', or 'scatter'."
        );
    }

    #[test]
    fn render_chart_rejects_zero_sum_pie() {
        let error = render_chart(ChartInput {
            chart_type: "pie".to_string(),
            data: r#"{"labels":["A","B"],"values":[0,0]}"#.to_string(),
            title: None,
            x_label: None,
            y_label: None,
        })
        .expect_err("pie should fail");

        assert_eq!(
            error.to_string(),
            "Pie chart values must sum to more than zero."
        );
    }

    #[test]
    fn render_document_supports_canvas_output() {
        let output = render_document(DocumentInput {
            sections: r#"[
                {"heading":"Overview","content":"Hello world","type":"text"},
                {"heading":"Example","content":"fn main() {}","type":"code","language":"rust"}
            ]"#
            .to_string(),
            title: Some("API Reference".to_string()),
            format: Some("canvas".to_string()),
        })
        .expect("document should render");
        let parsed: Value = serde_json::from_str(&output).expect("json output");

        assert_eq!(parsed["type"], "document");
        assert_eq!(parsed["canvas"]["kind"], "document");
        assert_eq!(parsed["canvas"]["sections"][1]["language"], "rust");
        assert!(parsed["markdown"]
            .as_str()
            .expect("markdown")
            .contains("```rust"));
    }

    #[test]
    fn render_document_supports_markdown_output() {
        let output = render_document(DocumentInput {
            sections: r#"[
                {"heading":"Overview","content":"Intro","type":"text"},
                {"heading":"Quote","content":"Stay curious","type":"quote"},
                {"heading":"Checklist","content":"One\nTwo","type":"list"}
            ]"#
            .to_string(),
            title: Some("Notes".to_string()),
            format: Some("markdown".to_string()),
        })
        .expect("markdown should render");
        let parsed: Value = serde_json::from_str(&output).expect("json output");
        let object = parsed.as_object().expect("document object");

        assert_eq!(parsed["status"], "success");
        assert_eq!(parsed["type"], "document");
        assert_eq!(parsed["message"], "📄 Document: Notes (3 sections)");
        assert_eq!(
            parsed["markdown"],
            "# Notes\n\n## Overview\n\nIntro\n\n## Quote\n\n> Stay curious\n\n## Checklist\n\n- One\n- Two"
        );
        assert!(!object.contains_key("canvas"));
    }

    #[test]
    fn render_document_defaults_section_type_to_text() {
        let sections = parse_sections(r#"[{"heading":"Overview","content":"Hello"}]"#)
            .expect("sections should parse");
        assert_eq!(sections[0].kind, SectionType::Text);
    }

    #[test]
    fn render_document_rejects_empty_sections() {
        let error = parse_sections("[]").expect_err("sections should fail");
        assert_eq!(error.to_string(), "Document sections must not be empty.");
    }

    #[test]
    fn render_document_rejects_empty_content() {
        let error = parse_sections(r#"[{"content":"   "}]"#).expect_err("content required");
        assert_eq!(error.to_string(), "Document sections must include content.");
    }

    #[test]
    fn render_document_rejects_invalid_format() {
        let error = OutputFormat::parse(Some("html")).expect_err("format should fail");
        assert_eq!(
            error.to_string(),
            "Invalid format 'html'. Use 'canvas' or 'markdown'."
        );
    }

    #[test]
    fn render_document_rejects_invalid_section_type() {
        let error =
            parse_sections(r#"[{"content":"Hello","type":"html"}]"#).expect_err("type should fail");
        assert_eq!(
            error.to_string(),
            "Invalid section type 'html'. Use 'text', 'code', 'quote', or 'list'."
        );
    }

    #[test]
    fn error_output_matches_contract() {
        let output = error_output(&CanvasError::InvalidInput("bad input".to_string()));
        assert_eq!(output, r#"{"error":"bad input"}"#);
    }

    #[test]
    fn manifest_matches_contract() {
        let manifest = include_str!("../manifest.toml");
        assert!(manifest.contains(r#"name = "canvas""#));
        assert!(manifest.contains(r#"capabilities = ["storage"]"#));
    }

    #[test]
    fn cargo_manifest_matches_contract() {
        let manifest = include_str!("../Cargo.toml");
        assert!(manifest.contains("strip = true"));
        assert!(manifest.contains("crate-type = [\"cdylib\"]"));
    }
}
