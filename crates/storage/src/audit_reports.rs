//! Frozen, deterministic audit evidence. No network or UI dependencies.
use crate::*;
use rusqlite::OpenFlags;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

pub const AUDIT_REPORT_SCHEMA_VERSION: u32 = 1;
pub const AUDIT_REPORT_RULE_VERSION: &str = "issue-view-v1";
pub const AUDIT_REPORT_PROMPT_VERSION: &str = "audit-annotations-v2";
pub const AUDIT_REPORT_PAGE_SIZE: usize = 100;
pub const AUDIT_REPORT_MAX_PAGE_SIZE: usize = 1_000;
pub const AUDIT_REPORT_MAX_PREVIEW_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AuditReportLanguage {
    English,
    Turkish,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AuditSourceStatus {
    Completed,
    Imported,
    Stopped,
    Failed,
    Running,
    Paused,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuditReportRequest {
    pub id: String,
    pub title: String,
    pub language: AuditReportLanguage,
    pub source_session_id: String,
    pub source_revision: String,
    pub source_status: AuditSourceStatus,
    pub created_at: String,
    pub scope: GridQuery,
    #[serde(default)]
    pub exclusions: Vec<String>,
    #[serde(default)]
    pub crawl_limits: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AuditCoverageState {
    Measured,
    Incomplete,
    NotMeasured,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditRuleCoverage {
    pub rule_id: String,
    pub state: AuditCoverageState,
    pub eligible_records: Option<usize>,
    pub reason: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditReportSummary {
    pub request: AuditReportRequest,
    pub schema_version: u32,
    pub rule_version: String,
    pub prompt_version: String,
    pub status: String,
    pub partial: bool,
    pub actual_source_revision: Option<i64>,
    pub source_records: usize,
    pub scope_records: usize,
    pub eligible_html_records: usize,
    pub blocked_records: usize,
    pub failed_records: usize,
    pub unavailable_html_records: usize,
    pub finding_count: usize,
    pub coverage: Vec<AuditRuleCoverage>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AuditFindingCounts {
    /// Original request URLs for page findings; destinations for link findings.
    pub unique_urls: usize,
    /// None when stored edges cannot identify their exact List/source occurrence.
    pub source_records: Option<usize>,
    pub source_pages: usize,
    pub occurrences: usize,
    pub targets: Option<usize>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditFinding {
    pub id: String,
    pub title: String,
    pub severity: Severity,
    pub category: String,
    pub counts: AuditFindingCounts,
    pub eligible_records: Option<usize>,
    pub coverage: AuditCoverageState,
    pub explanation: String,
    pub recommendation: String,
    pub verification: String,
    pub suggested_team: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AuditAttribution {
    ExactRecord,
    UrlOnly,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AuditEvidenceKind {
    Page,
    Link,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AuditObservedValues {
    pub status_code: Option<u16>,
    pub content_type: Option<String>,
    pub indexability_status: Option<String>,
    pub title: Option<String>,
    pub title_length: Option<usize>,
    pub meta_description: Option<String>,
    pub meta_description_length: Option<usize>,
    pub h1: Option<String>,
    pub canonical: Option<String>,
    pub anchor_text: Option<String>,
    pub rel: Option<String>,
    pub rendered: Option<bool>,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AuditEvidence {
    pub id: String,
    pub finding_id: String,
    pub source_storage_key: Option<String>,
    pub source_record_id: Option<u64>,
    pub list_position: Option<u32>,
    pub list_duplicate_index: Option<u32>,
    pub original_url: String,
    pub final_url: Option<String>,
    pub kind: AuditEvidenceKind,
    pub attribution: AuditAttribution,
    pub observed: AuditObservedValues,
    pub source_url: Option<String>,
    pub target_url: Option<String>,
    pub source_position: Option<u32>,
    pub source_edge_id: Option<u64>,
    /// Full values stay in SQLite; query callers explicitly choose bounded previews.
    pub preview_truncated: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuditEvidenceSort {
    #[default]
    Id,
    OriginalUrl,
    FinalUrl,
    TargetUrl,
    SourcePosition,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct AuditEvidenceQuery {
    pub finding_id: String,
    pub search: Option<String>,
    pub status_code: Option<u16>,
    pub sort_by: AuditEvidenceSort,
    pub sort_dir: SortDirection,
    pub offset: usize,
    pub limit: usize,
    pub preview: bool,
}
impl Default for AuditEvidenceQuery {
    fn default() -> Self {
        Self {
            finding_id: String::new(),
            search: None,
            status_code: None,
            sort_by: AuditEvidenceSort::Id,
            sort_dir: SortDirection::Asc,
            offset: 0,
            limit: AUDIT_REPORT_PAGE_SIZE,
            preview: true,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct AuditFindingQuery {
    pub search: Option<String>,
    pub severity: Option<Severity>,
    pub category: Option<String>,
    pub team: Option<String>,
    pub offset: usize,
    pub limit: usize,
}
impl Default for AuditFindingQuery {
    fn default() -> Self {
        Self {
            search: None,
            severity: None,
            category: None,
            team: None,
            offset: 0,
            limit: AUDIT_REPORT_PAGE_SIZE,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEvidenceResponse {
    pub rows: Vec<AuditEvidence>,
    pub total: usize,
}
/// Full evidence for internal export. `next_sequence` is the last emitted SQLite sequence.
pub struct AuditEvidenceExportWindow {
    pub rows: Vec<AuditEvidence>,
    pub next_sequence: Option<i64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditFindingResponse {
    pub rows: Vec<AuditFinding>,
    pub total: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditPreparationProgress {
    pub phase: String,
    pub completed: usize,
}
#[derive(Debug, Error)]
pub enum AuditReportError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("invalid audit report: {0}")]
    Invalid(String),
    #[error("audit report preparation cancelled")]
    Cancelled,
}
/// Read-only after publication; source edits and deletions cannot alter a report.
pub struct AuditReportStore {
    conn: Mutex<Connection>,
}

struct Rule {
    id: &'static str,
    title: &'static str,
    view: IssueView,
    category: &'static str,
    severity: Severity,
}
fn rules() -> Vec<Rule> {
    use IssueView::*;
    [
        (
            "title.missing",
            "Missing page title",
            TitleMissing,
            "Titles",
            Severity::Error,
        ),
        (
            "title.duplicate",
            "Duplicate page title",
            TitleDuplicate,
            "Titles",
            Severity::Warning,
        ),
        (
            "title.tooShort",
            "Short page title",
            TitleTooShort,
            "Titles",
            Severity::Warning,
        ),
        (
            "title.tooLong",
            "Long page title",
            TitleTooLong,
            "Titles",
            Severity::Warning,
        ),
        (
            "meta.missing",
            "Missing meta description",
            MetaMissing,
            "Descriptions",
            Severity::Warning,
        ),
        (
            "meta.duplicate",
            "Duplicate meta description",
            MetaDuplicate,
            "Descriptions",
            Severity::Warning,
        ),
        (
            "meta.tooShort",
            "Short meta description",
            MetaTooShort,
            "Descriptions",
            Severity::Warning,
        ),
        (
            "meta.tooLong",
            "Long meta description",
            MetaTooLong,
            "Descriptions",
            Severity::Warning,
        ),
        (
            "h1.missing",
            "Missing H1 heading",
            H1Missing,
            "Headings",
            Severity::Warning,
        ),
        (
            "canonical.missing",
            "Missing canonical",
            CanonicalMissing,
            "Canonicals",
            Severity::Warning,
        ),
        (
            "canonical.toError",
            "Canonical points to a failed target",
            CanonicalToError,
            "Canonicals",
            Severity::Error,
        ),
        (
            "canonical.toRedirect",
            "Canonical points to a redirect",
            CanonicalToRedirect,
            "Canonicals",
            Severity::Warning,
        ),
        (
            "canonical.loop",
            "Canonical loop",
            CanonicalLoop,
            "Canonicals",
            Severity::Error,
        ),
        (
            "response.clientError",
            "Client error response",
            Status4xx,
            "Response codes",
            Severity::Error,
        ),
        (
            "response.serverError",
            "Server error response",
            Status5xx,
            "Response codes",
            Severity::Error,
        ),
        (
            "response.noResponse",
            "No response",
            NoResponse,
            "Response codes",
            Severity::Error,
        ),
    ]
    .into_iter()
    .map(|(id, title, view, category, severity)| Rule {
        id,
        title,
        view,
        category,
        severity,
    })
    .collect()
}

impl AuditReportStore {
    /// Open saved evidence without running crawl-schema migrations or changing the active crawl.
    /// Legacy schemas missing required fields fail explicitly and remain untouched.
    pub fn prepare_saved_with_progress(
        path: impl AsRef<Path>,
        source_path: impl AsRef<Path>,
        request: AuditReportRequest,
        progress: impl FnMut(AuditPreparationProgress) -> bool,
    ) -> Result<Self, AuditReportError> {
        let conn = Connection::open_with_flags(source_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let source = SqliteStore {
            conn: Arc::new(Mutex::new(conn)),
            summary_cache: Default::default(),
            reference_cache: Default::default(),
            exact_duplicate_cache: Default::default(),
            image_alias_revision: Default::default(),
        };
        Self::prepare_with_progress(path, &ActiveStore::Sqlite(source), request, progress)
    }
    pub fn prepare(
        path: impl AsRef<Path>,
        source: &ActiveStore,
        request: AuditReportRequest,
    ) -> Result<Self, AuditReportError> {
        Self::prepare_with_progress(path, source, request, |_| true)
    }

    /// Runs synchronously on a native worker. Callbacks must not reenter the source store.
    /// Returning false cancels; a partially prepared report is never published.
    pub fn prepare_with_progress(
        path: impl AsRef<Path>,
        source: &ActiveStore,
        mut request: AuditReportRequest,
        mut progress: impl FnMut(AuditPreparationProgress) -> bool,
    ) -> Result<Self, AuditReportError> {
        validate_request(&request)?;
        // Window and ordering are presentation settings, never a report membership cap.
        request.scope.offset = 0;
        request.scope.limit = AUDIT_REPORT_PAGE_SIZE;
        let path = path.as_ref();
        if path.exists() {
            return Err(AuditReportError::Invalid(
                "destination already exists".into(),
            ));
        }
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let name = path
            .file_name()
            .ok_or_else(|| AuditReportError::Invalid("destination needs a filename".into()))?
            .to_string_lossy();
        let temporary = path.with_file_name(format!(
            ".{name}.preparing-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, AtomicOrdering::Relaxed)
        ));
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        let cleanup = TemporaryReport(temporary.clone());
        {
            let snapshot = SqliteStore::open(&temporary)?;
            snapshot
                .connection()?
                .execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;")?;
            create_tables(&*snapshot.connection()?)?;
            let revision = copy_source(source, &snapshot, &mut progress)?;
            build_report(&snapshot, request, revision, &mut progress)?;
            tick(&mut progress, "finalizing", 0)?;
            prune_unrelated_context(&*snapshot.connection()?)?;
        }
        tick(&mut progress, "publishing", 0)?;
        // A same-directory hard link atomically publishes without overwriting an existing report.
        std::fs::hard_link(&temporary, path)?;
        drop(cleanup);
        Self::open(path)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, AuditReportError> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let report = Self {
            conn: Mutex::new(conn),
        };
        let summary = report.summary()?;
        if summary.schema_version != AUDIT_REPORT_SCHEMA_VERSION || summary.status != "ready" {
            return Err(AuditReportError::Invalid(
                "unsupported schema or incomplete report".into(),
            ));
        }
        Ok(report)
    }

    pub(super) fn connection(&self) -> Result<MutexGuard<'_, Connection>, AuditReportError> {
        self.conn
            .lock()
            .map_err(|_| StorageError::LockPoisoned.into())
    }

    pub fn summary(&self) -> Result<AuditReportSummary, AuditReportError> {
        let payload: String = self.connection()?.query_row(
            "SELECT payload FROM audit_report WHERE id=1",
            [],
            |r| r.get(0),
        )?;
        Ok(serde_json::from_str(&payload)?)
    }

    pub fn query_findings(
        &self,
        query: AuditFindingQuery,
    ) -> Result<AuditFindingResponse, AuditReportError> {
        validate_page(query.offset, query.limit, query.search.as_deref())?;
        validate_page(0, 1, query.category.as_deref())?;
        validate_page(0, 1, query.team.as_deref())?;
        let conn = self.connection()?;
        let search = query.search.unwrap_or_default().to_lowercase();
        let severity = query
            .severity
            .map(|s| serde_json::to_string(&s))
            .transpose()?;
        let filter = " WHERE (?1='' OR instr(search_text,?1)>0) AND (?2 IS NULL OR severity=?2) AND (?3 IS NULL OR category=?3) AND (?4 IS NULL OR json_extract(payload,'$.suggestedTeam')=?4)";
        let args = params![search, severity, query.category, query.team];
        let total = conn.query_row(
            &format!("SELECT COUNT(*) FROM audit_findings{filter}"),
            args,
            |r| r.get::<_, i64>(0).map(|value| value as usize),
        )?;
        let mut statement = conn.prepare(&format!("SELECT payload FROM audit_findings{filter} ORDER BY severity_rank DESC,id ASC LIMIT {} OFFSET {}", query.limit, query.offset))?;
        let mut rows = Vec::new();
        for row in statement.query_map(args, |r| r.get::<_, String>(0))? {
            rows.push(serde_json::from_str(&row?)?);
        }
        Ok(AuditFindingResponse { rows, total })
    }

    pub fn query_evidence(
        &self,
        query: AuditEvidenceQuery,
    ) -> Result<AuditEvidenceResponse, AuditReportError> {
        validate_page(query.offset, query.limit, query.search.as_deref())?;
        if query
            .status_code
            .is_some_and(|code| !(100..=599).contains(&code))
        {
            return Err(AuditReportError::Invalid(
                "HTTP status must be between 100 and 599".into(),
            ));
        }
        let conn = self.connection()?;
        if !conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM audit_rules WHERE id=?1)",
            [&query.finding_id],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(AuditReportError::Invalid("unknown finding ID".into()));
        }
        let search = query.search.unwrap_or_default().to_lowercase();
        let filter = " WHERE finding_id=?1 AND (?2='' OR instr(search_text,?2)>0) AND (?3 IS NULL OR json_extract(payload,'$.observed.statusCode')=?3)";
        let args = params![query.finding_id, search, query.status_code];
        let total = if search.is_empty() && query.status_code.is_none() {
            // Findings and evidence are frozen together. Avoid recounting a million link rows
            // for every unfiltered UI window while retaining live counts for searched windows.
            conn.query_row(
                "SELECT COALESCE((SELECT json_extract(payload,'$.counts.occurrences')
                FROM audit_findings WHERE id=?1),0)",
                [&query.finding_id],
                |r| r.get::<_, i64>(0).map(|value| value as usize),
            )?
        } else {
            conn.query_row(
                &format!("SELECT COUNT(*) FROM audit_evidence{filter}"),
                args,
                |r| r.get::<_, i64>(0).map(|value| value as usize),
            )?
        };
        let column = match query.sort_by {
            AuditEvidenceSort::Id => "sequence",
            AuditEvidenceSort::OriginalUrl => "original_url",
            AuditEvidenceSort::FinalUrl => "final_url",
            AuditEvidenceSort::TargetUrl => "target_url",
            AuditEvidenceSort::SourcePosition => "source_position",
        };
        let direction = if query.sort_dir == SortDirection::Desc {
            "DESC"
        } else {
            "ASC"
        };
        let mut statement = conn.prepare(&format!("SELECT payload FROM audit_evidence{filter} ORDER BY {column} {direction},sequence ASC LIMIT {} OFFSET {}",query.limit,query.offset))?;
        let mut rows = Vec::new();
        for row in statement.query_map(args, |r| r.get::<_, String>(0))? {
            let mut evidence: AuditEvidence = serde_json::from_str(&row?)?;
            if query.preview {
                truncate_preview(&mut evidence);
            }
            rows.push(evidence);
        }
        Ok(AuditEvidenceResponse { rows, total })
    }

    /// Internal full-value sequence walk for offline export; the UI keeps its offset/sort API.
    pub fn export_evidence_window(
        &self,
        finding_id: &str,
        after_sequence: Option<i64>,
        limit: usize,
    ) -> Result<AuditEvidenceExportWindow, AuditReportError> {
        validate_page(0, limit, None)?;
        if after_sequence.is_some_and(|value| value < 0) {
            return Err(AuditReportError::Invalid(
                "invalid evidence export cursor".into(),
            ));
        }
        let conn = self.connection()?;
        if !conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM audit_rules WHERE id=?1)",
            [finding_id],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(AuditReportError::Invalid("unknown finding ID".into()));
        }
        let mut statement = conn.prepare(
            "SELECT sequence,payload FROM audit_evidence
            WHERE finding_id=?1 AND sequence>?2 ORDER BY sequence LIMIT ?3",
        )?;
        let mut rows = Vec::new();
        let mut next_sequence = None;
        for result in statement.query_map(
            params![finding_id, after_sequence.unwrap_or(0), limit as i64],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        )? {
            let (sequence, payload) = result?;
            rows.push(serde_json::from_str(&payload)?);
            next_sequence = Some(sequence);
        }
        Ok(AuditEvidenceExportWindow {
            rows,
            next_sequence,
        })
    }
}

pub(super) struct TemporaryReport(pub(super) PathBuf);
impl Drop for TemporaryReport {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        for suffix in ["-wal", "-shm", "-journal"] {
            let mut path = self.0.as_os_str().to_os_string();
            path.push(suffix);
            let _ = std::fs::remove_file(PathBuf::from(path));
        }
    }
}
fn validate_request(request: &AuditReportRequest) -> Result<(), AuditReportError> {
    if matches!(
        request.source_status,
        AuditSourceStatus::Running | AuditSourceStatus::Paused
    ) {
        return Err(AuditReportError::Invalid(
            "stop the crawl before preparing a report".into(),
        ));
    }
    for (name, value) in [
        ("id", &request.id),
        ("title", &request.title),
        ("sourceSessionId", &request.source_session_id),
        ("sourceRevision", &request.source_revision),
        ("createdAt", &request.created_at),
    ] {
        if value.trim().is_empty() || value.len() > 4096 {
            return Err(AuditReportError::Invalid(format!(
                "{name} must contain 1–4096 bytes"
            )));
        }
    }
    request
        .scope
        .thresholds
        .validate()
        .map_err(AuditReportError::Invalid)?;
    if request.scope.segment_regex
        && let Some(pattern) = &request.scope.segment_pattern
    {
        Regex::new(pattern).map_err(|error| {
            AuditReportError::Invalid(format!("invalid segment regex: {error}"))
        })?;
    }
    validate_grid_query(&request.scope)?;
    Ok(())
}
pub(super) fn validate_page(
    offset: usize,
    limit: usize,
    search: Option<&str>,
) -> Result<(), AuditReportError> {
    if !(1..=AUDIT_REPORT_MAX_PAGE_SIZE).contains(&limit)
        || offset > i64::MAX as usize
        || search.is_some_and(|s| s.len() > AUDIT_REPORT_MAX_PREVIEW_BYTES)
    {
        return Err(AuditReportError::Invalid(
            "invalid query window or search length".into(),
        ));
    }
    Ok(())
}
fn tick(
    progress: &mut impl FnMut(AuditPreparationProgress) -> bool,
    phase: &str,
    completed: usize,
) -> Result<(), AuditReportError> {
    if progress(AuditPreparationProgress {
        phase: phase.into(),
        completed,
    }) {
        Ok(())
    } else {
        Err(AuditReportError::Cancelled)
    }
}
fn create_tables(conn: &Connection) -> Result<(), AuditReportError> {
    conn.execute_batch("CREATE TABLE audit_report(id INTEGER PRIMARY KEY CHECK(id=1),payload TEXT NOT NULL);
        CREATE TABLE audit_sources(storage_key TEXT PRIMARY KEY,source_id INTEGER NOT NULL);
        CREATE TABLE audit_scope(record_id INTEGER PRIMARY KEY);
        CREATE TABLE audit_rules(id TEXT PRIMARY KEY);
        CREATE TABLE audit_findings(id TEXT PRIMARY KEY,severity TEXT NOT NULL,severity_rank INTEGER NOT NULL,category TEXT NOT NULL,search_text TEXT NOT NULL,payload TEXT NOT NULL);
        CREATE TABLE audit_evidence(sequence INTEGER PRIMARY KEY,id TEXT NOT NULL UNIQUE,finding_id TEXT NOT NULL,source_key TEXT,original_url TEXT NOT NULL,final_url TEXT,source_url TEXT,target_url TEXT,source_position INTEGER,search_text TEXT NOT NULL,payload TEXT NOT NULL);
        CREATE INDEX audit_evidence_finding ON audit_evidence(finding_id,sequence);
        CREATE INDEX audit_evidence_original ON audit_evidence(finding_id,original_url,sequence);
        CREATE INDEX audit_evidence_final ON audit_evidence(finding_id,final_url,sequence);
        CREATE INDEX audit_evidence_target ON audit_evidence(finding_id,target_url,sequence);")?;
    Ok(())
}
fn copy_source(
    source: &ActiveStore,
    target: &SqliteStore,
    progress: &mut impl FnMut(AuditPreparationProgress) -> bool,
) -> Result<Option<i64>, AuditReportError> {
    tick(progress, "snapshot", 0)?;
    target.connection()?.execute_batch("BEGIN IMMEDIATE")?;
    let mut count = 0;
    let mut copy_record = |record: CrawlRecord| -> Result<(), AuditReportError> {
        let original_id = record.id;
        let original_key = record.storage_key.clone();
        target.try_upsert(record)?;
        target.connection()?.execute(
            "INSERT INTO audit_sources VALUES(?1,?2)",
            params![original_key, original_id as i64],
        )?;
        count += 1;
        if count % AUDIT_REPORT_PAGE_SIZE == 0 {
            tick(progress, "snapshot", count)?;
        }
        Ok(())
    };
    let revision = match source {
        ActiveStore::Memory(store) => {
            let inner = store.inner.read().map_err(|_| StorageError::LockPoisoned)?;
            for record in &inner.records {
                copy_record(record.clone())?;
            }
            for (index, edge) in inner.link_edges.iter().enumerate() {
                copy_edge(target, edge)?;
                if index % AUDIT_REPORT_PAGE_SIZE == 0 {
                    tick(progress, "snapshotLinks", index)?;
                }
            }
            None
        }
        ActiveStore::Sqlite(store) => {
            let conn = store.connection()?;
            let transaction = conn.unchecked_transaction()?;
            let revision = crawl_audit_revision(&transaction)?;
            {
                let mut statement =
                    transaction.prepare("SELECT * FROM crawl_records ORDER BY id")?;
                for record in statement.query_map([], record_from_row)? {
                    copy_record(record?)?;
                }
            }
            {
                let mut statement = transaction.prepare("SELECT * FROM link_edges ORDER BY id")?;
                for (index, edge) in statement.query_map([], link_edge_from_row)?.enumerate() {
                    copy_edge(target, &edge?)?;
                    if index % AUDIT_REPORT_PAGE_SIZE == 0 {
                        tick(progress, "snapshotLinks", index)?;
                    }
                }
            }
            transaction.commit()?;
            Some(revision)
        }
    };
    target.connection()?.execute_batch("COMMIT")?;
    Ok(revision)
}
fn copy_edge(target: &SqliteStore, edge: &LinkEdge) -> Result<(), AuditReportError> {
    target.connection()?.execute("INSERT INTO link_edges(id,source_url,target_url,anchor_text,rel,rel_nofollow,link_type,source_status_code,target_status_code,source_depth,target_depth,source_position,discovery_order) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",params![edge.id as i64,edge.source_url,edge.target_url,edge.anchor_text,edge.rel,edge.rel_nofollow,link_type_to_str(&edge.link_type),edge.source_status_code,edge.target_status_code,edge.source_depth as i64,edge.target_depth.map(|v|v as i64),edge.source_position,edge.discovery_order as i64])?;
    Ok(())
}

fn build_report(
    snapshot: &SqliteStore,
    request: AuditReportRequest,
    revision: Option<i64>,
    progress: &mut impl FnMut(AuditPreparationProgress) -> bool,
) -> Result<(), AuditReportError> {
    tick(progress, "analysis", 0)?;
    // Build shared reference diagnostics once over the whole snapshot, before applying scope.
    snapshot.try_summary()?;
    let mut conn = snapshot.connection()?;
    let transaction = conn.transaction()?;
    let (filter, args) = query_filter_sql(&request.scope);
    let ctes = if matches!(
        request.scope.view,
        IssueView::HreflangMissingReturnLink | IssueView::HreflangNonCanonicalTarget
    ) {
        HREFLANG_AUDIT_CTES
    } else {
        ""
    };
    transaction.execute(
        &format!("{ctes}INSERT INTO audit_scope SELECT id FROM crawl_records{filter}"),
        rusqlite::params_from_iter(&args),
    )?;
    let count = |condition: &str| -> Result<usize, rusqlite::Error> {
        transaction.query_row(&format!("SELECT COUNT(*) FROM crawl_records WHERE id IN (SELECT record_id FROM audit_scope) AND ({condition})"),[],|r|r.get::<_, i64>(0).map(|value| value as usize))
    };
    let scope_records = count("1")?;
    let eligible_html_records = count(SUCCESS_HTML_SQL)?;
    let mut coverage = Vec::new();
    for (index, rule) in rules().into_iter().enumerate() {
        tick(progress, "findings", index)?;
        transaction.execute("INSERT INTO audit_rules VALUES(?1)", [rule.id])?;
        let query = GridQuery {
            view: rule.view.clone(),
            thresholds: request.scope.thresholds,
            ..Default::default()
        };
        let (filter, args) = query_filter_sql(&query);
        let conjunction = if filter.is_empty() {
            " WHERE "
        } else {
            " AND "
        };
        let mut statement=transaction.prepare(&format!("SELECT * FROM crawl_records{filter}{conjunction}id IN (SELECT record_id FROM audit_scope) ORDER BY id"))?;
        for (index, row) in statement
            .query_map(rusqlite::params_from_iter(&args), record_from_row)?
            .enumerate()
        {
            let row = row?;
            let source_id = transaction.query_row(
                "SELECT source_id FROM audit_sources WHERE storage_key=?1",
                [&row.storage_key],
                |r| r.get::<_, i64>(0).map(|value| value as usize),
            )?;
            insert_evidence(&transaction, page_evidence(&rule, &row, source_id as u64))?;
            if index % AUDIT_REPORT_PAGE_SIZE == 0 {
                tick(progress, "evidence", index)?;
            }
        }
        let eligible = if is_html_audit_view(&rule.view) {
            eligible_html_records
        } else {
            scope_records
        };
        save_finding(
            &transaction,
            &rule,
            Some(eligible),
            AuditCoverageState::Measured,
            false,
        )?;
        coverage.push(AuditRuleCoverage {rule_id:rule.id.into(),state:AuditCoverageState::Measured,eligible_records:Some(eligible),reason:"Evaluated from frozen captured records using the existing IssueView predicate; global context includes out-of-scope records.".into()});
    }
    build_broken_links(&transaction, progress)?;
    coverage.push(AuditRuleCoverage {rule_id:"links.broken".into(),state:AuditCoverageState::Incomplete,eligible_records:None,reason:"All retained matching edges are available. Legacy edges identify source URLs, not exact List occurrences; capture completeness and source-record counts cannot be proven.".into()});
    for (id, reason) in [
        (
            "otherIssueViews",
            "Only the explicitly listed report rules are evaluated; other workbench audit views are not measured in this report version.",
        ),
        (
            "imageOccurrences",
            "Image occurrence evidence is not included in this report version.",
        ),
        (
            "browserInteractions",
            "Forms, analytics events, accessibility interactions and legal compliance were not tested.",
        ),
        (
            "retainedBodies",
            "Raw HTML, rendered HTML, headers and retained text are not copied into the report; only captured record fields and link evidence are available.",
        ),
    ] {
        coverage.push(AuditRuleCoverage {
            rule_id: id.into(),
            state: AuditCoverageState::NotMeasured,
            eligible_records: None,
            reason: reason.into(),
        });
    }
    let summary = AuditReportSummary {
        partial: request.source_status != AuditSourceStatus::Completed,
        schema_version: AUDIT_REPORT_SCHEMA_VERSION,
        rule_version: AUDIT_REPORT_RULE_VERSION.into(),
        prompt_version: AUDIT_REPORT_PROMPT_VERSION.into(),
        status: "ready".into(),
        actual_source_revision: revision,
        source_records: transaction.query_row("SELECT COUNT(*) FROM crawl_records", [], |r| {
            r.get::<_, i64>(0).map(|v| v as usize)
        })?,
        scope_records,
        eligible_html_records,
        blocked_records: count(
            "lower(indexability_status) LIKE '%robots%' AND status_code IS NULL",
        )?,
        failed_records: count(&broken_record_sql())?,
        unavailable_html_records: count("indexability_status = 'Response body incomplete'")?,
        finding_count: transaction.query_row("SELECT COUNT(*) FROM audit_findings", [], |r| {
            r.get::<_, i64>(0).map(|v| v as usize)
        })?,
        coverage,
        request,
    };
    transaction.execute(
        "INSERT INTO audit_report VALUES(1,?1)",
        [serde_json::to_string(&summary)?],
    )?;
    transaction.commit()?;
    Ok(())
}
fn page_evidence(rule: &Rule, row: &CrawlRecord, source_id: u64) -> AuditEvidence {
    AuditEvidence {
        id: format!("{}:record:{source_id}", rule.id),
        finding_id: rule.id.into(),
        source_storage_key: Some(row.storage_key.clone()),
        source_record_id: Some(source_id),
        list_position: row.list_position,
        list_duplicate_index: Some(row.list_duplicate_index),
        original_url: row.url.clone(),
        final_url: Some(row.final_url.clone()),
        kind: AuditEvidenceKind::Page,
        attribution: AuditAttribution::ExactRecord,
        observed: AuditObservedValues {
            status_code: row.status_code,
            content_type: row.content_type.clone(),
            indexability_status: Some(row.indexability_status.clone()),
            title: row.title.clone(),
            title_length: Some(row.title_len),
            meta_description: row.meta_description.clone(),
            meta_description_length: Some(row.meta_description_len),
            h1: row.h1.clone(),
            canonical: row.canonical.clone(),
            rendered: Some(row.js_rendered),
            error: row.error.clone(),
            ..Default::default()
        },
        source_url: Some(row.url.clone()),
        target_url: if rule.category == "Canonicals" {
            row.canonical.clone()
        } else {
            None
        },
        source_position: None,
        source_edge_id: None,
        preview_truncated: false,
    }
}
fn insert_evidence(conn: &Connection, evidence: AuditEvidence) -> Result<(), AuditReportError> {
    let payload = serde_json::to_string(&evidence)?;
    // Search indexes every stored field, including off-page full values and exact source keys.
    conn.execute("INSERT INTO audit_evidence(id,finding_id,source_key,original_url,final_url,source_url,target_url,source_position,search_text,payload) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",params![evidence.id,evidence.finding_id,evidence.source_storage_key,evidence.original_url,evidence.final_url,evidence.source_url,evidence.target_url,evidence.source_position,payload.to_lowercase(),payload])?;
    Ok(())
}
fn save_finding(
    conn: &Connection,
    rule: &Rule,
    eligible: Option<usize>,
    coverage: AuditCoverageState,
    links: bool,
) -> Result<(), AuditReportError> {
    let (occurrences,urls,records,pages,targets):(usize,usize,usize,usize,usize)=conn.query_row("SELECT COUNT(*),COUNT(DISTINCT original_url),COUNT(DISTINCT source_key),COUNT(DISTINCT source_url),COUNT(DISTINCT target_url) FROM audit_evidence WHERE finding_id=?1",[rule.id],|r|Ok((r.get::<_, i64>(0)? as usize,r.get::<_, i64>(1)? as usize,r.get::<_, i64>(2)? as usize,r.get::<_, i64>(3)? as usize,r.get::<_, i64>(4)? as usize)))?;
    if occurrences == 0 {
        return Ok(());
    }
    let finding=AuditFinding {
        id:rule.id.into(),title:rule.title.into(),severity:rule.severity.clone(),category:rule.category.into(),counts:AuditFindingCounts {unique_urls:if links {targets} else {urls},source_records:if links {None} else {Some(records)},source_pages:pages,occurrences,targets:links.then_some(targets)},eligible_records:eligible,coverage,
        explanation:format!("{} was detected in the frozen crawl evidence. Counts describe captured observations within the saved report scope.",rule.title),
        recommendation:match rule.category {"Titles"=>"Review the affected titles and provide a descriptive, distinct title within the saved thresholds.","Descriptions"=>"Review the affected descriptions and provide a useful, distinct description within the saved thresholds.","Headings"=>"Provide a descriptive primary heading for the affected page.","Canonicals"=>"Check the canonical declaration and point it to the intended accessible, indexable destination.",_=>"Review the failed URL and its references; correct the destination or remove an obsolete reference."}.into(),
        verification:"Recrawl the affected URLs with comparable scope and thresholds, then inspect the newly captured evidence.".into(),suggested_team:if matches!(rule.category,"Titles"|"Descriptions"|"Headings") {"Content"} else {"Engineering"}.into(),
    };
    let rank = match rule.severity {
        Severity::Info => 0,
        Severity::Warning => 1,
        Severity::Error => 2,
    };
    conn.execute(
        "INSERT INTO audit_findings VALUES(?1,?2,?3,?4,?5,?6)",
        params![
            finding.id,
            serde_json::to_string(&finding.severity)?,
            rank,
            finding.category,
            format!("{} {} {}", finding.id, finding.title, finding.category).to_lowercase(),
            serde_json::to_string(&finding)?
        ],
    )?;
    Ok(())
}
fn build_broken_links(
    conn: &Connection,
    progress: &mut impl FnMut(AuditPreparationProgress) -> bool,
) -> Result<(), AuditReportError> {
    let rule = Rule {
        id: "links.broken",
        title: "Broken link destinations",
        view: IssueView::BrokenLinks,
        category: "Links",
        severity: Severity::Error,
    };
    conn.execute("INSERT INTO audit_rules VALUES(?1)", [rule.id])?;
    // Reuse existing failure and URL alias semantics, keeping global target context indexed.
    conn.execute_batch(&format!("CREATE TEMP TABLE audit_failed_aliases(alias TEXT PRIMARY KEY);
        INSERT OR IGNORE INTO audit_failed_aliases SELECT a.value FROM crawl_records r,json_each(ff_url_aliases(r.storage_key,r.url,r.final_url)) a WHERE {};
        CREATE TEMP TABLE audit_scope_aliases(alias TEXT PRIMARY KEY);
        INSERT OR IGNORE INTO audit_scope_aliases SELECT a.value FROM crawl_records r,json_each(ff_url_aliases(r.storage_key,r.url,r.final_url)) a WHERE r.id IN (SELECT record_id FROM audit_scope);",broken_record_sql()))?;
    let mut statement=conn.prepare("SELECT * FROM link_edges WHERE (target_status_code>=400 OR target_url IN (SELECT alias FROM audit_failed_aliases)) AND source_url IN (SELECT alias FROM audit_scope_aliases) ORDER BY id")?;
    for (index, edge) in statement.query_map([], link_edge_from_row)?.enumerate() {
        let edge = edge?;
        insert_evidence(
            conn,
            AuditEvidence {
                id: format!("links.broken:edge:{}", edge.id),
                finding_id: rule.id.into(),
                source_storage_key: None,
                source_record_id: None,
                list_position: None,
                list_duplicate_index: None,
                original_url: edge.source_url.clone(),
                final_url: None,
                kind: AuditEvidenceKind::Link,
                attribution: AuditAttribution::UrlOnly,
                observed: AuditObservedValues {
                    status_code: edge.target_status_code,
                    anchor_text: Some(edge.anchor_text),
                    rel: Some(edge.rel),
                    ..Default::default()
                },
                source_url: Some(edge.source_url),
                target_url: Some(edge.target_url),
                source_position: Some(edge.source_position),
                source_edge_id: Some(edge.id),
                preview_truncated: false,
            },
        )?;
        if index % AUDIT_REPORT_PAGE_SIZE == 0 {
            tick(progress, "linkEvidence", index)?;
        }
    }
    save_finding(conn, &rule, None, AuditCoverageState::Incomplete, true)
}
fn truncate_preview(evidence: &mut AuditEvidence) {
    evidence.preview_truncated = truncate_observed_values(&mut evidence.observed);
}

pub(super) fn truncate_observed_values(observed: &mut AuditObservedValues) -> bool {
    let mut truncated = false;
    for text in [
        &mut observed.content_type,
        &mut observed.indexability_status,
        &mut observed.title,
        &mut observed.meta_description,
        &mut observed.h1,
        &mut observed.canonical,
        &mut observed.anchor_text,
        &mut observed.rel,
        &mut observed.error,
    ]
    .into_iter()
    .flatten()
    {
        if text.len() > AUDIT_REPORT_MAX_PREVIEW_BYTES {
            let mut end = AUDIT_REPORT_MAX_PREVIEW_BYTES;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
            truncated = true;
        }
    }
    truncated
}

/// Retain negative evidence for this version's supported checks, not arbitrary source payloads.
/// New crawl columns are excluded unless deliberately added to this report projection.
fn prune_unrelated_context(conn: &Connection) -> Result<(), AuditReportError> {
    const RETAINED: &[&str] = &[
        "id",
        "storage_key",
        "url",
        "final_url",
        "list_position",
        "list_duplicate_index",
        "classification",
        "status_code",
        "status_text",
        "content_type",
        "indexability",
        "indexability_status",
        "redirect_target",
        "redirect_type",
        "redirect_chain",
        "title",
        "title_len",
        "meta_description",
        "meta_description_len",
        "h1",
        "canonical",
        "canonical_count",
        "js_rendered",
        "error",
    ];
    let assignments = {
        let mut statement = conn.prepare("PRAGMA table_info(crawl_records)")?;
        let columns = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, bool>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })?;
        let mut assignments = Vec::new();
        for column in columns {
            let (name, kind, required, default) = column?;
            if RETAINED.contains(&name.as_str()) {
                continue;
            }
            let value = default.unwrap_or_else(|| {
                if !required {
                    "NULL".into()
                } else if kind == "TEXT" {
                    "''".into()
                } else {
                    "0".into()
                }
            });
            assignments.push(format!("{} = {value}", sqlite_identifier(&name)));
        }
        assignments
    };
    conn.execute(
        &format!("UPDATE crawl_records SET {}", assignments.join(",")),
        [],
    )?;
    // DELETE/UPDATE alone leaves old payload bytes in free pages. Compact before publication.
    conn.execute_batch("VACUUM")?;
    Ok(())
}

#[cfg(test)]
mod export_cursor_tests {
    use super::*;

    #[test]
    fn full_value_cursor_reconciles_large_finding_and_matches_offset_first_middle_last() {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ff-audit-cursor-{}-{}.sqlite3",
            std::process::id(),
            NEXT.fetch_add(1, AtomicOrdering::Relaxed)
        ));
        let source = ActiveStore::memory();
        for index in 0..2_205 {
            let mut row = CrawlRecord::pending(format!("https://example.test/{index:04}"), 0);
            row.status_code = Some(200);
            row.content_type = Some("text/html".into());
            row.indexability_status = "Indexable".into();
            if index == 2_204 {
                row.h1 = Some("last full value ".to_owned() + &"x".repeat(70_000));
            }
            source.upsert(row);
        }
        let report = AuditReportStore::prepare(
            &path,
            &source,
            AuditReportRequest {
                id: "cursor-test".into(),
                title: "Cursor test".into(),
                language: AuditReportLanguage::English,
                source_session_id: "source".into(),
                source_revision: "revision".into(),
                source_status: AuditSourceStatus::Completed,
                created_at: "2026-09-14T00:00:00Z".into(),
                scope: GridQuery::default(),
                exclusions: vec![],
                crawl_limits: vec![],
            },
        )
        .unwrap();
        let finding_id = "title.missing";
        let mut cursor = None;
        let mut ids = Vec::new();
        while ids.len() < 2_205 {
            let window = report
                .export_evidence_window(finding_id, cursor, 100)
                .unwrap();
            assert_eq!(window.rows.len(), (2_205 - ids.len()).min(100));
            cursor = window.next_sequence;
            ids.extend(window.rows.into_iter().map(|row| row.id));
        }
        assert!(
            report
                .export_evidence_window(finding_id, cursor, 1)
                .unwrap()
                .rows
                .is_empty()
        );
        assert_eq!(
            ids.iter().collect::<std::collections::HashSet<_>>().len(),
            2_205
        );
        for offset in [0, 1_102, 2_204] {
            let page = report
                .query_evidence(AuditEvidenceQuery {
                    finding_id: finding_id.into(),
                    offset,
                    limit: 1,
                    preview: false,
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(page.total, 2_205);
            assert_eq!(page.rows[0].id, ids[offset]);
            if offset == 2_204 {
                assert!(page.rows[0].observed.h1.as_deref().unwrap().len() > 70_000);
            }
        }
        assert!(
            report
                .export_evidence_window(finding_id, Some(-1), 1)
                .is_err()
        );
        assert!(
            report
                .export_evidence_window("not-a-rule", None, 1)
                .is_err()
        );
        let connection = Connection::open(&path).unwrap();
        let plan: String = connection
            .query_row(
                "EXPLAIN QUERY PLAN SELECT sequence,payload FROM audit_evidence
            WHERE finding_id=?1 AND sequence>?2 ORDER BY sequence LIMIT 100",
                rusqlite::params![finding_id, 0],
                |row| row.get(3),
            )
            .unwrap();
        assert!(plan.contains("audit_evidence_finding"), "{plan}");
        drop(connection);
        drop(report);
        std::fs::remove_file(path).unwrap();
    }
}
