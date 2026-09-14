//! Persisted comparisons of frozen audit reports. Missing evidence never verifies a fix.
use crate::*;
use rusqlite::OpenFlags;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

pub const AUDIT_COMPARISON_SCHEMA_VERSION: u32 = 1;
pub const AUDIT_COMPARISON_IDENTITY_VERSION: &str = "request-url-occurrence-v1";
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AuditComparisonStatus {
    New,
    Resolved,
    Improved,
    Unchanged,
    Worsened,
    MixedChanges,
    NotComparable,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AuditComparisonEvidenceState {
    Added,
    Persisting,
    Resolved,
    NotObserved,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditComparisonObservation {
    pub evidence_id: Option<String>,
    pub source_storage_key: Option<String>,
    pub source_record_id: Option<u64>,
    pub list_position: Option<u32>,
    pub list_duplicate_index: Option<u32>,
    pub original_url: String,
    pub final_url: Option<String>,
    pub source_url: Option<String>,
    pub target_url: Option<String>,
    pub source_position: Option<u32>,
    pub source_edge_id: Option<u64>,
    pub kind: AuditEvidenceKind,
    pub attribution: AuditAttribution,
    pub observed: AuditObservedValues,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditComparisonEvidence {
    pub id: u64,
    pub finding_id: String,
    pub identity_key: String,
    pub request_url: String,
    pub occurrence: usize,
    pub state: AuditComparisonEvidenceState,
    pub reason: String,
    pub baseline: Option<AuditComparisonObservation>,
    pub current: Option<AuditComparisonObservation>,
    pub preview_truncated: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditComparisonFinding {
    pub finding_id: String,
    pub title: String,
    pub severity: Severity,
    pub category: String,
    pub status: AuditComparisonStatus,
    pub baseline_counts: Option<AuditFindingCounts>,
    pub current_counts: Option<AuditFindingCounts>,
    pub added: usize,
    pub persisting: usize,
    pub resolved: usize,
    pub not_observed: usize,
    pub newly_observed_current: usize,
    pub unverified_current: usize,
    pub count_unit: String,
    pub compatibility_reasons: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditComparisonSummary {
    pub schema_version: u32,
    pub identity_version: String,
    pub status: String,
    pub baseline_report_id: String,
    pub current_report_id: String,
    pub baseline: AuditReportSummary,
    pub current: AuditReportSummary,
    pub finding_count: usize,
    pub evidence_rows: usize,
    pub compatibility_reasons: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct AuditComparisonFindingQuery {
    pub status: Option<AuditComparisonStatus>,
    pub search: Option<String>,
    pub offset: usize,
    pub limit: usize,
}
impl Default for AuditComparisonFindingQuery {
    fn default() -> Self {
        Self {
            status: None,
            search: None,
            offset: 0,
            limit: AUDIT_REPORT_PAGE_SIZE,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct AuditComparisonEvidenceQuery {
    pub finding_id: String,
    pub state: Option<AuditComparisonEvidenceState>,
    pub search: Option<String>,
    pub sort_dir: SortDirection,
    pub offset: usize,
    pub limit: usize,
    pub preview: bool,
}
impl Default for AuditComparisonEvidenceQuery {
    fn default() -> Self {
        Self {
            finding_id: String::new(),
            state: None,
            search: None,
            sort_dir: SortDirection::Asc,
            offset: 0,
            limit: AUDIT_REPORT_PAGE_SIZE,
            preview: true,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditComparisonFindingResponse {
    pub rows: Vec<AuditComparisonFinding>,
    pub total: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditComparisonEvidenceResponse {
    pub rows: Vec<AuditComparisonEvidence>,
    pub total: usize,
}
/// Last exported row in the default request-URL/occurrence order.
#[derive(Clone, Debug)]
pub struct AuditComparisonExportCursor {
    pub request_url: String,
    pub occurrence: i64,
    pub id: i64,
}
/// Full-value internal export window, independent of UI offsets and previews.
pub struct AuditComparisonExportWindow {
    pub rows: Vec<AuditComparisonEvidence>,
    pub next_cursor: Option<AuditComparisonExportCursor>,
}
pub struct AuditReportComparisonStore {
    conn: Mutex<Connection>,
}

impl AuditReportComparisonStore {
    /// Materialize an independent comparison. Run on a native worker; false cancels preparation.
    pub fn prepare(
        path: impl AsRef<Path>,
        baseline: &AuditReportStore,
        current: &AuditReportStore,
        mut progress: impl FnMut(AuditPreparationProgress) -> bool,
    ) -> Result<Self, AuditReportError> {
        let path = path.as_ref();
        if path.exists() {
            return Err(AuditReportError::Invalid(
                "comparison destination already exists".into(),
            ));
        }
        let baseline_summary = baseline.summary()?;
        let current_summary = current.summary()?;
        let reasons = compatibility(&baseline_summary, &current_summary)?;
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let name = path
            .file_name()
            .ok_or_else(|| {
                AuditReportError::Invalid("comparison destination needs a filename".into())
            })?
            .to_string_lossy();
        let temporary = path.with_file_name(format!(
            ".{name}.comparing-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, AtomicOrdering::Relaxed)
        ));
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        let cleanup = crate::audit_reports::TemporaryReport(temporary.clone());
        {
            let mut conn = Connection::open(&temporary)?;
            conn.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;")?;
            attach(&conn, baseline, "baseline")?;
            attach(&conn, current, "current")?;
            register_identity(&conn)?;
            let transaction = conn.transaction()?;
            tick(&mut progress, "identity", 0)?;
            create_records(&transaction, "baseline")?;
            create_records(&transaction, "current")?;
            transaction.execute_batch("CREATE TABLE comparison_evidence(id INTEGER PRIMARY KEY,finding_id TEXT NOT NULL,identity_key TEXT NOT NULL,request_url TEXT NOT NULL,occurrence INTEGER NOT NULL,state TEXT NOT NULL,newly_observed_current INTEGER NOT NULL,unverified_current INTEGER NOT NULL,reason TEXT NOT NULL,search_text TEXT NOT NULL,payload TEXT NOT NULL);
                CREATE INDEX comparison_evidence_finding ON comparison_evidence(finding_id,request_url,occurrence,id);
                CREATE INDEX comparison_evidence_state ON comparison_evidence(finding_id,state,request_url,occurrence,id);
                CREATE TABLE comparison_findings(id TEXT PRIMARY KEY,status TEXT NOT NULL,search_text TEXT NOT NULL,payload TEXT NOT NULL);
                CREATE TABLE comparison_summary(id INTEGER PRIMARY KEY CHECK(id=1),payload TEXT NOT NULL);")?;
            let baseline_context = observed_context(&transaction, "baseline", "current")?;
            let current_context = observed_context(&transaction, "current", "baseline")?;
            let ids = {
                let mut statement=transaction.prepare("SELECT id FROM baseline.audit_findings UNION SELECT id FROM current.audit_findings ORDER BY id")?;
                statement
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            for (index, id) in ids.iter().enumerate() {
                tick(&mut progress, "findings", index)?;
                let previous = source_finding(&transaction, "baseline", id)?;
                let next = source_finding(&transaction, "current", id)?;
                let mut rule_reasons = reasons.clone();
                if id == "links.broken" {
                    rule_reasons.push("Link edges lack exact source occurrence attribution; retained reference sets are not comparable.".into());
                    copy_unattributed_edges(&transaction, id, &mut progress)?;
                } else {
                    if !rule_measured(&baseline_summary, id) || !rule_measured(&current_summary, id)
                    {
                        rule_reasons.push(
                            "The rule was not measured with compatible coverage in both reports."
                                .into(),
                        );
                    }
                    compare_rule(
                        &transaction,
                        id,
                        &rule_reasons,
                        baseline_context,
                        current_context,
                        &mut progress,
                    )?;
                }
                save_comparison_finding(&transaction, id, previous, next, rule_reasons)?;
            }
            let summary = AuditComparisonSummary {
                schema_version: AUDIT_COMPARISON_SCHEMA_VERSION,
                identity_version: AUDIT_COMPARISON_IDENTITY_VERSION.into(),
                status: "ready".into(),
                baseline_report_id: baseline_summary.request.id.clone(),
                current_report_id: current_summary.request.id.clone(),
                baseline: baseline_summary,
                current: current_summary,
                finding_count: ids.len(),
                evidence_rows: transaction.query_row(
                    "SELECT COUNT(*) FROM comparison_evidence",
                    [],
                    |r| r.get::<_, i64>(0).map(|v| v as usize),
                )?,
                compatibility_reasons: reasons,
            };
            transaction.execute(
                "INSERT INTO comparison_summary VALUES(1,?1)",
                [serde_json::to_string(&summary)?],
            )?;
            // Observations are now embedded per comparison row; no source report dependency remains.
            transaction
                .execute_batch("DROP TABLE baseline_records; DROP TABLE current_records;")?;
            transaction.commit()?;
            conn.execute_batch("DETACH DATABASE baseline; DETACH DATABASE current; VACUUM;")?;
        }
        tick(&mut progress, "publishing", 0)?;
        std::fs::hard_link(&temporary, path)?;
        drop(cleanup);
        Self::open(path)
    }
    pub fn open(path: impl AsRef<Path>) -> Result<Self, AuditReportError> {
        let store = Self {
            conn: Mutex::new(Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?),
        };
        let summary = store.summary()?;
        if summary.schema_version != AUDIT_COMPARISON_SCHEMA_VERSION || summary.status != "ready" {
            return Err(AuditReportError::Invalid(
                "unsupported or incomplete comparison".into(),
            ));
        }
        Ok(store)
    }
    fn connection(&self) -> Result<MutexGuard<'_, Connection>, AuditReportError> {
        self.conn
            .lock()
            .map_err(|_| StorageError::LockPoisoned.into())
    }
    pub fn summary(&self) -> Result<AuditComparisonSummary, AuditReportError> {
        let payload: String = self.connection()?.query_row(
            "SELECT payload FROM comparison_summary WHERE id=1",
            [],
            |r| r.get(0),
        )?;
        Ok(serde_json::from_str(&payload)?)
    }
    pub fn query_findings(
        &self,
        query: AuditComparisonFindingQuery,
    ) -> Result<AuditComparisonFindingResponse, AuditReportError> {
        crate::audit_reports::validate_page(query.offset, query.limit, query.search.as_deref())?;
        let conn = self.connection()?;
        let state = query
            .status
            .map(|value| serde_json::to_string(&value))
            .transpose()?;
        let search = query.search.unwrap_or_default().to_lowercase();
        let filter = " WHERE (?1 IS NULL OR status=?1) AND (?2='' OR instr(search_text,?2)>0)";
        let args = params![state, search];
        let total = conn.query_row(
            &format!("SELECT COUNT(*) FROM comparison_findings{filter}"),
            args,
            |r| r.get::<_, i64>(0).map(|v| v as usize),
        )?;
        let mut statement = conn.prepare(&format!(
            "SELECT payload FROM comparison_findings{filter} ORDER BY id LIMIT {} OFFSET {}",
            query.limit, query.offset
        ))?;
        let mut rows = Vec::new();
        for payload in statement.query_map(args, |r| r.get::<_, String>(0))? {
            rows.push(serde_json::from_str(&payload?)?);
        }
        Ok(AuditComparisonFindingResponse { rows, total })
    }
    pub fn query_evidence(
        &self,
        query: AuditComparisonEvidenceQuery,
    ) -> Result<AuditComparisonEvidenceResponse, AuditReportError> {
        crate::audit_reports::validate_page(query.offset, query.limit, query.search.as_deref())?;
        let conn = self.connection()?;
        if !conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM comparison_findings WHERE id=?1)",
            [&query.finding_id],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(AuditReportError::Invalid(
                "unknown comparison finding".into(),
            ));
        }
        let state = query
            .state
            .map(|value| serde_json::to_string(&value))
            .transpose()?;
        let search = query.search.unwrap_or_default().to_lowercase();
        let args = params![query.finding_id, state, search];
        let filter = " WHERE finding_id=?1 AND (?2 IS NULL OR state=?2) AND (?3='' OR instr(search_text,?3)>0)";
        let total = if state.is_none() && search.is_empty() {
            conn.query_row(
                "SELECT json_extract(payload,'$.added')+json_extract(payload,'$.persisting')+
                json_extract(payload,'$.resolved')+json_extract(payload,'$.notObserved')
                FROM comparison_findings WHERE id=?1",
                [&query.finding_id],
                |r| r.get::<_, i64>(0).map(|value| value as usize),
            )?
        } else {
            conn.query_row(
                &format!("SELECT COUNT(*) FROM comparison_evidence{filter}"),
                args,
                |r| r.get::<_, i64>(0).map(|value| value as usize),
            )?
        };
        let direction = if query.sort_dir == SortDirection::Desc {
            "DESC"
        } else {
            "ASC"
        };
        let mut statement=conn.prepare(&format!("SELECT payload FROM comparison_evidence{filter} ORDER BY request_url {direction},occurrence ASC,id ASC LIMIT {} OFFSET {}",query.limit,query.offset))?;
        let mut rows = Vec::new();
        for payload in statement.query_map(args, |r| r.get::<_, String>(0))? {
            let mut row: AuditComparisonEvidence = serde_json::from_str(&payload?)?;
            if query.preview {
                for observation in [&mut row.baseline, &mut row.current].into_iter().flatten() {
                    row.preview_truncated |=
                        crate::audit_reports::truncate_observed_values(&mut observation.observed);
                }
            }
            rows.push(row);
        }
        Ok(AuditComparisonEvidenceResponse { rows, total })
    }

    /// Indexed full-value walk for complete package export. UI sorting/search stay unchanged.
    pub fn export_evidence_window(
        &self,
        finding_id: &str,
        after: Option<&AuditComparisonExportCursor>,
        limit: usize,
    ) -> Result<AuditComparisonExportWindow, AuditReportError> {
        crate::audit_reports::validate_page(0, limit, None)?;
        let conn = self.connection()?;
        if !conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM comparison_findings WHERE id=?1)",
            [finding_id],
            |row| row.get::<_, bool>(0),
        )? {
            return Err(AuditReportError::Invalid(
                "unknown comparison finding".into(),
            ));
        }
        let mut statement = if after.is_some() {
            conn.prepare(
                "SELECT request_url,occurrence,id,payload FROM comparison_evidence
                WHERE finding_id=?1 AND (request_url,occurrence,id)>(?2,?3,?4)
                ORDER BY request_url,occurrence,id LIMIT ?5",
            )?
        } else {
            conn.prepare(
                "SELECT request_url,occurrence,id,payload FROM comparison_evidence
                WHERE finding_id=?1 ORDER BY request_url,occurrence,id LIMIT ?2",
            )?
        };
        let read = |row: &rusqlite::Row<'_>| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        };
        let mut rows = Vec::new();
        let mut next_cursor = None;
        if let Some(after) = after {
            for result in statement.query_map(
                params![
                    finding_id,
                    after.request_url,
                    after.occurrence,
                    after.id,
                    limit as i64
                ],
                read,
            )? {
                let (request_url, occurrence, id, payload) = result?;
                rows.push(serde_json::from_str(&payload)?);
                next_cursor = Some(AuditComparisonExportCursor {
                    request_url,
                    occurrence,
                    id,
                });
            }
        } else {
            for result in statement.query_map(params![finding_id, limit as i64], read)? {
                let (request_url, occurrence, id, payload) = result?;
                rows.push(serde_json::from_str(&payload)?);
                next_cursor = Some(AuditComparisonExportCursor {
                    request_url,
                    occurrence,
                    id,
                });
            }
        }
        Ok(AuditComparisonExportWindow { rows, next_cursor })
    }
}
fn attach(
    conn: &Connection,
    report: &AuditReportStore,
    schema: &str,
) -> Result<(), AuditReportError> {
    let path = report
        .connection()?
        .path()
        .map(PathBuf::from)
        .ok_or_else(|| AuditReportError::Invalid("comparison requires a saved report".into()))?
        .canonicalize()?;
    let mut uri = url::Url::from_file_path(path)
        .map_err(|_| AuditReportError::Invalid("invalid source report path".into()))?;
    uri.set_query(Some("mode=ro"));
    conn.execute(&format!("ATTACH DATABASE ?1 AS {schema}"), [uri.as_str()])?;
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
fn compatibility(
    baseline: &AuditReportSummary,
    current: &AuditReportSummary,
) -> Result<Vec<String>, AuditReportError> {
    let mut reasons = Vec::new();
    if baseline.schema_version != current.schema_version
        || baseline.rule_version != current.rule_version
    {
        reasons.push("Report schema or rule versions differ.".into());
    }
    let membership = |summary: &AuditReportSummary| -> Result<serde_json::Value, AuditReportError> {
        let mut query = summary.request.scope.clone();
        query.offset = 0;
        query.limit = 100;
        query.sort_by = None;
        query.sort_dir = SortDirection::Asc;
        Ok(serde_json::to_value(query)?)
    };
    if membership(baseline)? != membership(current)? {
        reasons.push("Saved scope, filters, or audit thresholds differ.".into());
    }
    if baseline.request.exclusions != current.request.exclusions {
        reasons.push("Recorded scope exclusions differ.".into());
    }
    Ok(reasons)
}
fn rule_measured(summary: &AuditReportSummary, id: &str) -> bool {
    summary
        .coverage
        .iter()
        .any(|rule| rule.rule_id == id && rule.state == AuditCoverageState::Measured)
}
fn source_finding(
    conn: &Connection,
    side: &str,
    id: &str,
) -> Result<Option<AuditFinding>, AuditReportError> {
    let payload: Option<String> = conn
        .query_row(
            &format!("SELECT payload FROM {side}.audit_findings WHERE id=?1"),
            [id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(payload
        .map(|value| serde_json::from_str(&value))
        .transpose()?)
}

// Matches the established native comparison request URL / List position policy.
fn list_key(key: &str) -> Option<(u32, &str)> {
    let (position, url) = key.strip_prefix("list:")?.split_once(':')?;
    Some((position.parse().ok()?, url))
}
fn request_url(original: &str, key: &str, final_url: &str) -> String {
    let stored = list_key(key).map(|(_, url)| url).unwrap_or(key);
    let value = if !original.trim().is_empty() {
        original
    } else if url::Url::parse(stored).is_ok_and(|url| matches!(url.scheme(), "http" | "https")) {
        stored
    } else {
        final_url
    };
    match url::Url::parse(value.trim()) {
        Ok(mut url) => {
            url.set_fragment(None);
            url.to_string()
        }
        Err(_) => value.trim().to_string(),
    }
}
fn register_identity(conn: &Connection) -> Result<(), AuditReportError> {
    let flags = rusqlite::functions::FunctionFlags::SQLITE_UTF8
        | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC;
    conn.create_scalar_function("ff_report_request", 3, flags, |context| {
        Ok(request_url(
            &context.get::<String>(0)?,
            &context.get::<String>(1)?,
            &context.get::<String>(2)?,
        ))
    })?;
    conn.create_scalar_function("ff_report_position", 3, flags, |context| {
        Ok(context
            .get::<Option<i64>>(0)?
            .or_else(|| {
                list_key(&context.get::<String>(1).ok()?).map(|(position, _)| i64::from(position))
            })
            .unwrap_or(context.get::<i64>(2)?))
    })?;
    Ok(())
}
fn create_records(conn: &Connection, side: &str) -> Result<(), AuditReportError> {
    conn.execute_batch(&format!("CREATE TABLE {side}_records AS WITH inputs AS MATERIALIZED (
        SELECT r.*,s.source_id,ff_report_request(r.url,r.storage_key,r.final_url) AS request_url,ff_report_position(r.list_position,r.storage_key,r.id) AS identity_position,
            r.id IN (SELECT record_id FROM {side}.audit_scope) AS in_scope FROM {side}.crawl_records r JOIN {side}.audit_sources s USING(storage_key)
        ), groups AS (SELECT request_url,COUNT(*) AS population,MIN(list_duplicate_index)>0 AND COUNT(DISTINCT list_duplicate_index)=COUNT(*) AS known FROM inputs GROUP BY request_url)
        SELECT r.id,r.storage_key,r.request_url,CASE WHEN g.known THEN r.list_duplicate_index ELSE ROW_NUMBER() OVER(PARTITION BY r.request_url ORDER BY r.identity_position,r.storage_key,r.id) END AS occurrence,
            g.population>1 AND NOT g.known AS ambiguous,r.in_scope,
            ({SUCCESS_HTML_SQL}) AS html_eligible,r.status_code>=200 AND r.status_code<300 AND r.indexability_status!='Response body incomplete' AS success,
            r.status_code IS NOT NULL AND r.indexability_status!='Response body incomplete' AS observed_valid,r.js_rendered AS rendered,
            json_object('evidenceId',NULL,'sourceStorageKey',r.storage_key,'sourceRecordId',r.source_id,'listPosition',r.list_position,'listDuplicateIndex',r.list_duplicate_index,'originalUrl',r.url,'finalUrl',r.final_url,'sourceUrl',r.url,'targetUrl',NULL,'sourcePosition',NULL,'sourceEdgeId',NULL,'kind','page','attribution','exactRecord',
            'observed',json_object('statusCode',r.status_code,'contentType',r.content_type,'indexabilityStatus',r.indexability_status,'title',r.title,'titleLength',r.title_len,'metaDescription',r.meta_description,'metaDescriptionLength',r.meta_description_len,'h1',r.h1,'canonical',r.canonical,'anchorText',NULL,'rel',NULL,'rendered',json(CASE WHEN r.js_rendered THEN 'true' ELSE 'false' END),'error',r.error)) AS observation
        FROM inputs r JOIN groups g USING(request_url);
        CREATE UNIQUE INDEX {side}_identity ON {side}_records(request_url,occurrence);
        CREATE UNIQUE INDEX {side}_storage_key ON {side}_records(storage_key);"))?;
    Ok(())
}
fn observed_context(
    conn: &Connection,
    previous: &str,
    next: &str,
) -> Result<bool, AuditReportError> {
    Ok(conn.query_row(&format!("SELECT NOT EXISTS(SELECT 1 FROM {previous}_records p LEFT JOIN {next}_records c USING(request_url,occurrence) WHERE c.id IS NULL OR NOT c.observed_valid OR p.ambiguous OR c.ambiguous OR (p.html_eligible AND (NOT c.html_eligible OR p.rendered!=c.rendered)))"),[],|r|r.get(0))?)
}
struct ComparedRecord {
    url: String,
    occurrence: usize,
    baseline: Option<AuditComparisonObservation>,
    current: Option<AuditComparisonObservation>,
    previous_issue: bool,
    current_issue: bool,
    previous_scope: bool,
    current_scope: bool,
    previous_eligible: bool,
    current_eligible: bool,
    ambiguous: bool,
    capture_changed: bool,
}
fn compare_rule(
    conn: &Connection,
    id: &str,
    reasons: &[String],
    baseline_context: bool,
    current_context: bool,
    progress: &mut impl FnMut(AuditPreparationProgress) -> bool,
) -> Result<(), AuditReportError> {
    let html = !id.starts_with("response.");
    let eligible = if html { "html_eligible" } else { "success" };
    let mut statement=conn.prepare(&format!("WITH previous_evidence AS (SELECT * FROM baseline.audit_evidence WHERE finding_id=?1 AND source_key IS NOT NULL), next_evidence AS (SELECT * FROM current.audit_evidence WHERE finding_id=?1 AND source_key IS NOT NULL), affected AS (
        SELECT r.request_url,r.occurrence FROM previous_evidence e JOIN baseline_records r ON r.storage_key=e.source_key UNION SELECT r.request_url,r.occurrence FROM next_evidence e JOIN current_records r ON r.storage_key=e.source_key)
        SELECT a.request_url,a.occurrence,
        CASE WHEN b.id IS NOT NULL THEN json_set(b.observation,'$.evidenceId',p.id) END,CASE WHEN c.id IS NOT NULL THEN json_set(c.observation,'$.evidenceId',n.id) END,
        p.id IS NOT NULL,n.id IS NOT NULL,COALESCE(b.in_scope,0),COALESCE(c.in_scope,0),COALESCE(b.{eligible},0),COALESCE(c.{eligible},0),COALESCE(b.ambiguous,0) OR COALESCE(c.ambiguous,0),COALESCE(b.rendered!=c.rendered,0)
        FROM affected a LEFT JOIN baseline_records b USING(request_url,occurrence) LEFT JOIN current_records c USING(request_url,occurrence) LEFT JOIN previous_evidence p ON p.source_key=b.storage_key LEFT JOIN next_evidence n ON n.source_key=c.storage_key ORDER BY a.request_url,a.occurrence"))?;
    let rows = statement.query_map([id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, bool>(4)?,
            r.get::<_, bool>(5)?,
            r.get::<_, bool>(6)?,
            r.get::<_, bool>(7)?,
            r.get::<_, bool>(8)?,
            r.get::<_, bool>(9)?,
            r.get::<_, bool>(10)?,
            r.get::<_, bool>(11)?,
        ))
    })?;
    for (index, row) in rows.enumerate() {
        let (
            url,
            occurrence,
            b,
            c,
            previous_issue,
            current_issue,
            previous_scope,
            current_scope,
            previous_eligible,
            current_eligible,
            ambiguous,
            capture_changed,
        ) = row?;
        let record = ComparedRecord {
            url,
            occurrence: occurrence as usize,
            baseline: b.map(|v| serde_json::from_str(&v)).transpose()?,
            current: c.map(|v| serde_json::from_str(&v)).transpose()?,
            previous_issue,
            current_issue,
            previous_scope,
            current_scope,
            previous_eligible,
            current_eligible,
            ambiguous,
            capture_changed: html && capture_changed,
        };
        let (state, reason) = classify(&record, id, reasons, baseline_context, current_context);
        let newly_observed = state == AuditComparisonEvidenceState::NotObserved
            && record.current_issue
            && (record.baseline.is_none() || !record.previous_scope || !record.previous_eligible);
        let unverified = state == AuditComparisonEvidenceState::NotObserved && record.current_issue;
        insert_row(
            conn,
            AuditComparisonEvidence {
                id: 0,
                finding_id: id.into(),
                identity_key: format!("{}:{}", record.occurrence, record.url),
                request_url: record.url,
                occurrence: record.occurrence,
                state,
                reason,
                baseline: record.baseline,
                current: record.current,
                preview_truncated: false,
            },
            newly_observed,
            unverified,
        )?;
        if index % AUDIT_REPORT_PAGE_SIZE == 0 {
            tick(progress, "evidence", index)?;
        }
    }
    Ok(())
}
fn classify(
    record: &ComparedRecord,
    id: &str,
    reasons: &[String],
    baseline_context: bool,
    current_context: bool,
) -> (AuditComparisonEvidenceState, String) {
    use AuditComparisonEvidenceState::*;
    let unknown = |reason: &str| (NotObserved, reason.into());
    if !reasons.is_empty() {
        return (NotObserved, reasons.join(" "));
    }
    if record.baseline.is_none() {
        return unknown(
            "Not observed in the baseline; a current finding is not a verified regression.",
        );
    }
    if record.current.is_none() {
        return unknown("Not observed in the current report; absence does not verify a fix.");
    }
    if !record.previous_scope || !record.current_scope {
        return unknown("The matching occurrence is outside one report's saved scope.");
    }
    if record.ambiguous {
        return unknown("Legacy duplicate List occurrences cannot be matched reliably.");
    }
    if record.capture_changed {
        return unknown("HTTP and rendered capture modes differ.");
    }
    if record.previous_issue && record.current_issue {
        return (
            Persisting,
            "The same versioned rule remains detected for this request occurrence.".into(),
        );
    }
    let global = id.ends_with(".duplicate")
        || matches!(
            id,
            "canonical.toError" | "canonical.toRedirect" | "canonical.loop"
        );
    if record.previous_issue {
        if !record.current_eligible {
            return unknown(
                "The current occurrence is failed, blocked, incomplete, or ineligible; a fix is unverified.",
            );
        }
        if global && !baseline_context {
            return unknown(
                "Global duplicate/reference context from the baseline was not completely observed in the current report.",
            );
        }
        (
            Resolved,
            "Eligible current evidence verifies that the previous rule failure is absent.".into(),
        )
    } else {
        if !record.previous_eligible {
            return unknown(
                "The baseline occurrence is failed, blocked, incomplete, or ineligible; a regression is unverified.",
            );
        }
        if global && !current_context {
            return unknown(
                "Global duplicate/reference context from the current report was not completely observed in the baseline.",
            );
        }
        (Added,"Eligible baseline evidence was unaffected; the current occurrence now fails this rule.".into())
    }
}
fn insert_row(
    conn: &Connection,
    mut row: AuditComparisonEvidence,
    newly: bool,
    unverified: bool,
) -> Result<(), AuditReportError> {
    conn.execute("INSERT INTO comparison_evidence(finding_id,identity_key,request_url,occurrence,state,newly_observed_current,unverified_current,reason,search_text,payload) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'','')",params![row.finding_id,row.identity_key,row.request_url,row.occurrence as i64,serde_json::to_string(&row.state)?,newly,unverified,row.reason])?;
    row.id = conn.last_insert_rowid() as u64;
    let payload = serde_json::to_string(&row)?;
    conn.execute(
        "UPDATE comparison_evidence SET payload=?1,search_text=?2 WHERE id=?3",
        params![payload, payload.to_lowercase(), row.id as i64],
    )?;
    Ok(())
}
fn copy_unattributed_edges(
    conn: &Connection,
    id: &str,
    progress: &mut impl FnMut(AuditPreparationProgress) -> bool,
) -> Result<(), AuditReportError> {
    for side in ["baseline", "current"] {
        let mut statement = conn.prepare(&format!(
            "SELECT payload FROM {side}.audit_evidence WHERE finding_id=?1 ORDER BY sequence"
        ))?;
        for (index, payload) in statement
            .query_map([id], |r| r.get::<_, String>(0))?
            .enumerate()
        {
            let evidence: AuditEvidence = serde_json::from_str(&payload?)?;
            let identity = format!("{side}:{}", evidence.id);
            let url = evidence.original_url.clone();
            let observation = AuditComparisonObservation {
                evidence_id: Some(evidence.id),
                source_storage_key: evidence.source_storage_key,
                source_record_id: evidence.source_record_id,
                list_position: evidence.list_position,
                list_duplicate_index: evidence.list_duplicate_index,
                original_url: evidence.original_url,
                final_url: evidence.final_url,
                source_url: evidence.source_url,
                target_url: evidence.target_url,
                source_position: evidence.source_position,
                source_edge_id: evidence.source_edge_id,
                kind: evidence.kind,
                attribution: evidence.attribution,
                observed: evidence.observed,
            };
            let (baseline, current) = if side == "baseline" {
                (Some(observation), None)
            } else {
                (None, Some(observation))
            };
            insert_row(conn,AuditComparisonEvidence{id:0,finding_id:id.into(),identity_key:identity,request_url:url,occurrence:index+1,state:AuditComparisonEvidenceState::NotObserved,reason:"Exact legacy link occurrence correspondence and capture completeness are unavailable.".into(),baseline,current,preview_truncated:false},false,side=="current")?;
            if index % AUDIT_REPORT_PAGE_SIZE == 0 {
                tick(progress, "links", index)?;
            }
        }
    }
    Ok(())
}
fn save_comparison_finding(
    conn: &Connection,
    id: &str,
    baseline: Option<AuditFinding>,
    current: Option<AuditFinding>,
    mut reasons: Vec<String>,
) -> Result<(), AuditReportError> {
    let count = |state: AuditComparisonEvidenceState| -> Result<usize, AuditReportError> {
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM comparison_evidence WHERE finding_id=?1 AND state=?2",
            params![id, serde_json::to_string(&state)?],
            |r| r.get::<_, i64>(0).map(|v| v as usize),
        )?)
    };
    let added = count(AuditComparisonEvidenceState::Added)?;
    let persisting = count(AuditComparisonEvidenceState::Persisting)?;
    let resolved = count(AuditComparisonEvidenceState::Resolved)?;
    let not_observed = count(AuditComparisonEvidenceState::NotObserved)?;
    let status = if not_observed > 0 || !reasons.is_empty() {
        AuditComparisonStatus::NotComparable
    } else if added > 0 && resolved > 0 {
        AuditComparisonStatus::MixedChanges
    } else if resolved > 0 && persisting == 0 {
        AuditComparisonStatus::Resolved
    } else if resolved > 0 {
        AuditComparisonStatus::Improved
    } else if added > 0 && persisting == 0 {
        AuditComparisonStatus::New
    } else if added > 0 {
        AuditComparisonStatus::Worsened
    } else {
        AuditComparisonStatus::Unchanged
    };
    if not_observed > 0 {
        let mut statement=conn.prepare("SELECT DISTINCT reason FROM comparison_evidence WHERE finding_id=?1 AND state='\"notObserved\"' ORDER BY reason")?;
        for reason in statement.query_map([id], |r| r.get::<_, String>(0))? {
            let reason = reason?;
            if !reasons.contains(&reason) {
                reasons.push(reason);
            }
        }
    }
    let finding = current
        .as_ref()
        .or(baseline.as_ref())
        .ok_or_else(|| AuditReportError::Invalid("comparison rule has no finding".into()))?;
    let (newly_observed_current,unverified_current)=conn.query_row("SELECT COALESCE(SUM(newly_observed_current),0),COALESCE(SUM(unverified_current),0) FROM comparison_evidence WHERE finding_id=?1",[id],|r|Ok((r.get::<_,i64>(0)? as usize,r.get::<_,i64>(1)? as usize)))?;
    let row = AuditComparisonFinding {
        finding_id: id.into(),
        title: finding.title.clone(),
        severity: finding.severity.clone(),
        category: finding.category.clone(),
        status,
        baseline_counts: baseline.map(|f| f.counts),
        current_counts: current.map(|f| f.counts),
        added,
        persisting,
        resolved,
        not_observed,
        newly_observed_current,
        unverified_current,
        count_unit: if id == "links.broken" {
            "retained reference occurrences"
        } else {
            "request URL / List occurrences"
        }
        .into(),
        compatibility_reasons: reasons,
    };
    let payload = serde_json::to_string(&row)?;
    conn.execute(
        "INSERT INTO comparison_findings VALUES(?1,?2,?3,?4)",
        params![
            id,
            serde_json::to_string(&row.status)?,
            payload.to_lowercase(),
            payload
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod export_cursor_tests {
    use super::*;

    fn report(directory: &Path, id: &str, missing_title: bool) -> AuditReportStore {
        let source = ActiveStore::memory();
        for index in 0..2_205 {
            let mut row = CrawlRecord::pending(format!("https://example.test/{index:04}"), 0);
            row.status_code = Some(200);
            row.content_type = Some("text/html".into());
            row.indexability_status = "Indexable".into();
            row.meta_description = Some("Useful description".into());
            row.meta_description_len = 18;
            row.h1 = Some(if index == 2_204 {
                "x".repeat(70_000)
            } else {
                "Primary heading".into()
            });
            row.canonical = Some(row.url.clone());
            if !missing_title {
                row.title = Some(format!("Useful title {index}"));
                row.title_len = 17;
            }
            source.upsert(row);
        }
        AuditReportStore::prepare(
            directory.join(format!("{id}.sqlite3")),
            &source,
            AuditReportRequest {
                id: id.into(),
                title: id.into(),
                language: AuditReportLanguage::English,
                source_session_id: id.into(),
                source_revision: "revision".into(),
                source_status: AuditSourceStatus::Completed,
                created_at: "2026-09-14T00:00:00Z".into(),
                scope: GridQuery::default(),
                exclusions: vec![],
                crawl_limits: vec![],
            },
        )
        .unwrap()
    }

    #[test]
    fn cursor_reconciles_2205_rows_with_first_middle_last_ui_order_and_full_values() {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "ff-comparison-cursor-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, AtomicOrdering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let baseline = report(&directory, "baseline", true);
        let current = report(&directory, "current", false);
        let comparison_path = directory.join("comparison.sqlite3");
        let comparison =
            AuditReportComparisonStore::prepare(&comparison_path, &baseline, &current, |_| true)
                .unwrap();
        let finding = comparison
            .query_findings(Default::default())
            .unwrap()
            .rows
            .into_iter()
            .find(|row| row.finding_id == "title.missing")
            .unwrap();
        let total = finding.added + finding.persisting + finding.resolved + finding.not_observed;
        assert_eq!(total, 2_205);
        let mut cursor = None;
        let mut ids = Vec::new();
        while ids.len() < total {
            let window = comparison
                .export_evidence_window("title.missing", cursor.as_ref(), 100)
                .unwrap();
            assert_eq!(window.rows.len(), (total - ids.len()).min(100));
            cursor = window.next_cursor;
            ids.extend(window.rows.into_iter().map(|row| row.id));
        }
        assert!(
            comparison
                .export_evidence_window("title.missing", cursor.as_ref(), 1)
                .unwrap()
                .rows
                .is_empty()
        );
        assert_eq!(
            ids.iter().collect::<std::collections::HashSet<_>>().len(),
            total
        );
        for offset in [0, 1_102, 2_204] {
            let page = comparison
                .query_evidence(AuditComparisonEvidenceQuery {
                    finding_id: "title.missing".into(),
                    offset,
                    limit: 1,
                    preview: false,
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(page.total, total);
            assert_eq!(page.rows[0].id, ids[offset]);
            if offset == 2_204 {
                assert_eq!(
                    page.rows[0]
                        .baseline
                        .as_ref()
                        .unwrap()
                        .observed
                        .h1
                        .as_deref()
                        .unwrap()
                        .len(),
                    70_000
                );
            }
        }
        let connection = Connection::open(&comparison_path).unwrap();
        let plan: String = connection
            .query_row(
                "EXPLAIN QUERY PLAN SELECT request_url,occurrence,id,payload
            FROM comparison_evidence WHERE finding_id=?1 AND (request_url,occurrence,id)>(?2,?3,?4)
            ORDER BY request_url,occurrence,id LIMIT 100",
                params!["title.missing", "https://example.test/0000", 1, 0],
                |row| row.get(3),
            )
            .unwrap();
        assert!(plan.contains("comparison_evidence_finding"), "{plan}");
        drop(connection);
        drop(comparison);
        drop(baseline);
        drop(current);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
