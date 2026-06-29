use crate::error::{Error, Result};
use crate::model::{
    Chunk, ChunkRecord, Direction, EDGE_CONFIDENCE_EXTRACTED, EDGE_TYPE_CALLS, IndexReport,
    RelatedHit, RetrievalStrategy, Retrieved, SearchHeader, SearchHit, SourceRefresh, SourceSpan,
};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

const MIGRATION_001: &str = include_str!("../migrations/001_initial.sql");
const MIGRATION_002: &str = include_str!("../migrations/002_fts5.sql");
const MIGRATION_003: &str = include_str!("../migrations/003_embeddings.sql");
const MIGRATION_004: &str = include_str!("../migrations/004_source_extractor.sql");
const MIGRATION_005: &str = include_str!("../migrations/005_graph_edges.sql");
pub(crate) const LATEST_SCHEMA_VERSION: i64 = 5;

#[derive(Clone, Copy)]
struct IndexMode {
    force: bool,
    prune_missing: bool,
}

/// A stored embedding with the cheap keys needed to rank it, but without the
/// chunk body. Bodies are hydrated only for the ranked winners.
pub(crate) struct EmbeddingVector {
    pub(crate) chunk_id: String,
    pub(crate) source_path: String,
    pub(crate) byte_start: usize,
    pub(crate) vector: Vec<f32>,
}

pub(crate) fn index_corpus(root: &Path, database: &Path) -> Result<IndexReport> {
    index_corpus_inner(root, database, false)
}

/// Re-indexes every discovered source even when its content is unchanged, so a
/// schema or extractor upgrade (such as the call graph) backfills onto an
/// existing corpus. This is the forced full re-index behind `mycelia refresh`.
pub(crate) fn reindex_corpus(root: &Path, database: &Path) -> Result<IndexReport> {
    index_corpus_inner(root, database, true)
}

pub(crate) fn refresh_changed_sources<I, P>(
    root: &Path,
    database: &Path,
    relative_paths: I,
) -> Result<IndexReport>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let started_at = Instant::now();
    let canonical_root = canonicalize_root(root)?;
    let files = relative_paths
        .into_iter()
        .map(|path| canonical_root.join(path.as_ref()))
        .collect::<Vec<_>>();
    let mut report = index_corpus_with_files(
        canonical_root.as_path(),
        database,
        files,
        0,
        crate::extract::extract_text,
        crate::extract::extractor_id_for,
        IndexMode {
            force: false,
            prune_missing: false,
        },
    )?;
    report.elapsed_ms = started_at.elapsed().as_millis();
    Ok(report)
}

fn index_corpus_inner(root: &Path, database: &Path, force: bool) -> Result<IndexReport> {
    let started_at = Instant::now();
    let canonical_root = canonicalize_root(root)?;
    let mut discovery = crate::discovery::discover(canonical_root.as_path())?;
    let database_path = absolute_path(database)?;
    discovery
        .files
        .retain(|path| !is_database_artifact(path, database_path.as_path()));

    let mut report = index_corpus_with_files(
        canonical_root.as_path(),
        database,
        discovery.files,
        discovery.rejected,
        crate::extract::extract_text,
        crate::extract::extractor_id_for,
        IndexMode {
            force,
            prune_missing: true,
        },
    )?;
    report.elapsed_ms = started_at.elapsed().as_millis();
    Ok(report)
}

pub(crate) fn find(
    database: &Path,
    query: &str,
    limit: usize,
    strategy: RetrievalStrategy,
) -> Result<Vec<SearchHit>> {
    let normalized_query = query.trim();
    if normalized_query.is_empty() {
        return Err(Error::EmptyQuery);
    }
    if limit == 0 {
        return Err(Error::InvalidLimit);
    }

    match strategy {
        RetrievalStrategy::Substring => find_substring(database, normalized_query, limit),
        RetrievalStrategy::Fts5 => find_fts5(database, normalized_query, limit),
        RetrievalStrategy::Fts5Reranked => find_fts5_reranked(database, normalized_query, limit),
        RetrievalStrategy::Vector | RetrievalStrategy::Hybrid | RetrievalStrategy::Routed => {
            Err(Error::EmbeddingProvider(
                "vector, hybrid, and routed retrieval require an embedding provider".to_owned(),
            ))
        }
    }
}

fn find_substring(database: &Path, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
    let connection = open_database(database, Access::ReadOnly)?;
    let lowercase_query = query.to_lowercase();
    let like_pattern = format!("%{lowercase_query}%");
    let mut statement = connection.prepare(
        "SELECT id, source_path, source_hash, byte_start, byte_end, line_start, line_end, text, extractor, symbol
         FROM chunks
         WHERE lower(text) LIKE ?1
         ORDER BY source_path ASC, byte_start ASC, id ASC",
    )?;

    let rows = statement.query_map([like_pattern], chunk_record_from_row)?;
    let mut hits = Vec::new();
    for row in rows {
        let chunk = row?;
        let score = lexical_score(&chunk.text, &lowercase_query);
        if score > 0.0 {
            hits.push(SearchHit { chunk, score });
        }
    }

    hits.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.chunk.source_path.cmp(&right.chunk.source_path))
            .then_with(|| left.chunk.span.byte_start.cmp(&right.chunk.span.byte_start))
            .then_with(|| left.chunk.id.cmp(&right.chunk.id))
    });
    hits.truncate(limit);

    Ok(hits)
}

fn find_fts5(database: &Path, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
    find_fts5_candidates(database, query, limit)
}

fn find_fts5_reranked(database: &Path, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
    let query_sequence = tokenize(query);
    let query_terms = unique_tokens(query_sequence.as_slice())?;
    // Identifier-like query tokens, lowercased. `tokenize` discards case, so
    // derive symbol shape from the raw query. Only these terms feed the
    // signature-coverage signal, keeping it inert for plain prose words.
    let symbol_terms = query
        .split(|character: char| !(character.is_alphanumeric() || character == '_'))
        .filter(|token| crate::is_identifier_token(token))
        .map(str::to_lowercase)
        .collect::<std::collections::BTreeSet<_>>();
    let candidate_limit = limit.saturating_mul(20).clamp(500, 5_000);
    let candidates = find_fts5_candidates(database, query, candidate_limit)?;
    let mut ranked = candidates
        .into_iter()
        .map(|mut hit| {
            let text_tokens = tokenize(hit.chunk.text.as_str());
            let signature_line = hit
                .chunk
                .text
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or_default();
            let signature_tokens = tokenize(signature_line);
            let exact_phrase =
                contains_token_sequence(text_tokens.as_slice(), query_sequence.as_slice());
            let coverage = query_terms
                .iter()
                .filter(|token| text_tokens.contains(token))
                .count();
            let coverage_ratio = coverage as f64 / query_terms.len() as f64;
            // Signature coverage: symbol-shaped query terms appearing in the
            // chunk's leading line. A definition carries the queried symbol in
            // its signature (`export class RunStore`, `pub fn find`), while a
            // reference buries it in the body, so this lifts definitions above
            // call sites. Gated to identifier tokens so prose terms in a
            // paragraph's first line never distort ranking.
            let signature_coverage = symbol_terms
                .iter()
                .filter(|token| signature_tokens.contains(*token))
                .count();
            let signature_ratio = signature_coverage as f64 / query_terms.len() as f64;
            let bm25_component = hit.score / (1.0 + hit.score.abs());
            hit.score = if exact_phrase { 2.0 } else { 0.0 }
                + coverage_ratio
                + signature_ratio
                + bm25_component;
            (hit, exact_phrase, coverage, signature_coverage)
        })
        .collect::<Vec<_>>();

    ranked.sort_by(
        |(left, left_phrase, left_coverage, _left_signature),
         (right, right_phrase, right_coverage, _right_signature)| {
            right_phrase
                .cmp(left_phrase)
                .then_with(|| right_coverage.cmp(left_coverage))
                .then_with(|| right.score.total_cmp(&left.score))
                .then_with(|| left.chunk.source_path.cmp(&right.chunk.source_path))
                .then_with(|| left.chunk.span.byte_start.cmp(&right.chunk.span.byte_start))
                .then_with(|| left.chunk.id.cmp(&right.chunk.id))
        },
    );
    Ok(truncate_unique_text_hits(
        ranked
            .into_iter()
            .map(|(hit, _, _, _)| hit)
            .collect::<Vec<_>>(),
        limit,
    ))
}

pub(crate) fn truncate_unique_text_hits(hits: Vec<SearchHit>, limit: usize) -> Vec<SearchHit> {
    let mut seen = BTreeSet::new();
    let mut unique = Vec::new();
    for hit in hits {
        if seen.insert(hit.chunk.text.clone()) {
            unique.push(hit);
            if unique.len() == limit {
                break;
            }
        }
    }
    unique
}

fn find_fts5_candidates(database: &Path, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
    let fts_query = fts5_query(query)?;
    let connection = open_database(database, Access::ReadOnly)?;
    let mut statement = connection.prepare(
        "SELECT
            chunks.id,
            chunks.source_path,
            chunks.source_hash,
            chunks.byte_start,
            chunks.byte_end,
            chunks.line_start,
            chunks.line_end,
            chunks.text,
            chunks.extractor,
            chunks.symbol,
            -bm25(chunk_fts) AS score
         FROM chunk_fts
         JOIN chunks ON chunks.rowid = chunk_fts.rowid
         WHERE chunk_fts MATCH ?1
         ORDER BY
            bm25(chunk_fts) ASC,
            chunks.source_path ASC,
            chunks.byte_start ASC,
            chunks.id ASC
         LIMIT ?2",
    )?;
    let rows = statement.query_map(
        params![fts_query, usize_to_i64(limit)?],
        |row| -> rusqlite::Result<SearchHit> {
            Ok(SearchHit {
                chunk: chunk_record_from_row(row)?,
                score: row.get(10)?,
            })
        },
    )?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Error::from)
}

pub(crate) fn retrieve(database: &Path, chunk_id: &str) -> Result<Option<Retrieved>> {
    let connection = open_database(database, Access::ReadOnly)?;

    let record = connection
        .query_row(
            "SELECT id, source_path, source_hash, byte_start, byte_end, line_start, line_end, text, extractor, symbol
             FROM chunks
             WHERE id = ?1",
            [chunk_id],
            chunk_record_from_row,
        )
        .optional()?;

    let Some(record) = record else {
        return Ok(None);
    };

    let stored_root = connection
        .query_row(
            "SELECT value FROM metadata WHERE key = 'corpus_root'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or(Error::MissingCorpusRoot)?;

    Ok(Some(validate_freshness(stored_root.as_str(), record)))
}

/// Standard chunk column list (indices 0..=9) prefixed with the chunks table,
/// for joins that also select edge columns.
const CHUNK_COLUMNS_PREFIXED: &str = "chunks.id, chunks.source_path, chunks.source_hash, \
     chunks.byte_start, chunks.byte_end, chunks.line_start, chunks.line_end, chunks.text, \
     chunks.extractor, chunks.symbol";

pub(crate) fn find_relationships(
    database: &Path,
    symbol: &str,
    direction: Direction,
) -> Result<Vec<RelatedHit>> {
    let connection = open_database(database, Access::ReadOnly)?;
    match direction {
        Direction::Callers => relationship_callers(&connection, symbol),
        Direction::Callees => relationship_callees(&connection, symbol),
    }
}

/// Chunks that call `symbol`. Each hit is a real call site; `resolved` reflects
/// whether the queried name maps to exactly one in-corpus definition, so a caller
/// of an ambiguous (or external) name is surfaced honestly rather than silently
/// attributed to one definition.
fn relationship_callers(connection: &Connection, symbol: &str) -> Result<Vec<RelatedHit>> {
    let definition_count = symbol_definition_count(connection, symbol)?;
    if definition_count == 0 {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT {CHUNK_COLUMNS_PREFIXED},
                edges.byte_start, edges.byte_end, edges.line_start, edges.line_end
         FROM edges
         JOIN chunks ON chunks.id = edges.src_chunk_id
         WHERE edges.dst_symbol = ?1 AND edges.edge_type = ?2
         ORDER BY chunks.source_path ASC, chunks.byte_start ASC, edges.byte_start ASC"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params![symbol, EDGE_TYPE_CALLS], |row| {
        Ok((chunk_record_from_row(row)?, span_from_row(row, 10)?))
    })?;

    let mut hits = Vec::new();
    for row in rows {
        let (record, call_site) = row?;
        hits.push(RelatedHit {
            symbol: record.symbol.clone().unwrap_or_default(),
            definition: SearchHeader::from_record(&record, 0.0),
            call_site,
            resolved: definition_count == 1,
            definition_count,
        });
    }
    Ok(hits)
}

/// Definitions of the symbols that `symbol` calls. A callee name with no
/// in-corpus definition (a std or external call) resolves to nothing and is
/// omitted; an ambiguous name yields one hit per candidate with `resolved` false.
fn relationship_callees(connection: &Connection, symbol: &str) -> Result<Vec<RelatedHit>> {
    let source_ids = {
        let mut statement = connection.prepare(
            "SELECT id FROM chunks WHERE symbol = ?1 ORDER BY source_path ASC, byte_start ASC",
        )?;
        let rows = statement.query_map([symbol], |row| row.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<String>>>()?
    };

    let mut hits = Vec::new();
    for source_id in &source_ids {
        let call_sites = {
            let mut statement = connection.prepare(
                "SELECT dst_symbol, byte_start, byte_end, line_start, line_end
                 FROM edges
                 WHERE src_chunk_id = ?1 AND edge_type = ?2
                 ORDER BY byte_start ASC",
            )?;
            let rows = statement.query_map(params![source_id, EDGE_TYPE_CALLS], |row| {
                Ok((row.get::<_, String>(0)?, span_from_row(row, 1)?))
            })?;
            rows.collect::<rusqlite::Result<Vec<(String, SourceSpan)>>>()?
        };

        for (callee, call_site) in call_sites {
            let definitions = load_chunks_by_symbol(connection, callee.as_str())?;
            let definition_count = definitions.len();
            for definition in &definitions {
                hits.push(RelatedHit {
                    symbol: callee.clone(),
                    definition: SearchHeader::from_record(definition, 0.0),
                    call_site: call_site.clone(),
                    resolved: definition_count == 1,
                    definition_count,
                });
            }
        }
    }
    Ok(hits)
}

/// Returns headers for every chunk in `changed_paths` plus the callers and
/// callees of every named symbol those paths define — the "blast radius" of a
/// diff. Deduplicates by chunk id. Changed-path chunks score 1.0; callers /
/// callees outside the changed set score 0.5. Sorted by source_path then
/// line_start, capped at `limit`.
pub(crate) fn blast_radius(
    database: &Path,
    changed_paths: &[String],
    limit: usize,
) -> Result<Vec<SearchHeader>> {
    if changed_paths.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let connection = open_database(database, Access::ReadOnly)?;

    // --- 1. Collect headers for chunks in the changed paths (score 1.0) ---
    let mut by_id: BTreeMap<String, SearchHeader> = BTreeMap::new();
    for batch in changed_paths.chunks(500) {
        let placeholders = vec!["?"; batch.len()].join(", ");
        let sql = format!(
            "SELECT id, source_path, source_hash, byte_start, byte_end, \
             line_start, line_end, text, extractor, symbol \
             FROM chunks WHERE source_path IN ({placeholders}) \
             ORDER BY source_path ASC, byte_start ASC"
        );
        let mut stmt = connection.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(batch), chunk_record_from_row)?;
        for row in rows {
            let record = row?;
            by_id
                .entry(record.id.clone())
                .or_insert_with(|| SearchHeader::from_record(&record, 1.0));
        }
    }

    // --- 2. Collect unique symbols defined in the changed paths ---
    let mut symbols: Vec<String> = Vec::new();
    for batch in changed_paths.chunks(500) {
        let placeholders = vec!["?"; batch.len()].join(", ");
        let sql = format!(
            "SELECT DISTINCT symbol FROM chunks \
             WHERE source_path IN ({placeholders}) AND symbol IS NOT NULL"
        );
        let mut stmt = connection.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(batch), |row| row.get(0))?;
        for row in rows {
            symbols.push(row?);
        }
    }
    symbols.sort();
    symbols.dedup();

    // --- 3. Expand callers and callees for each changed symbol (score 0.5) ---
    let insert_related = |by_id: &mut BTreeMap<String, SearchHeader>, hit: RelatedHit| {
        by_id
            .entry(hit.definition.chunk_id.clone())
            .or_insert_with(|| {
                let mut h = hit.definition;
                h.score = 0.5;
                h
            });
    };
    for symbol in &symbols {
        for hit in relationship_callers(&connection, symbol)? {
            insert_related(&mut by_id, hit);
        }
        for hit in relationship_callees(&connection, symbol)? {
            insert_related(&mut by_id, hit);
        }
    }

    // --- 4. Sort by path then line, cap at limit ---
    let mut result: Vec<SearchHeader> = by_id.into_values().collect();
    result.sort_by(|a, b| {
        a.source_path
            .cmp(&b.source_path)
            .then(a.span.line_start.cmp(&b.span.line_start))
    });
    result.truncate(limit);
    Ok(result)
}

fn symbol_definition_count(connection: &Connection, symbol: &str) -> Result<usize> {
    let count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM chunks WHERE symbol = ?1",
        [symbol],
        |row| row.get(0),
    )?;
    Ok(usize::try_from(count).unwrap_or(0))
}

fn load_chunks_by_symbol(connection: &Connection, symbol: &str) -> Result<Vec<ChunkRecord>> {
    let mut statement = connection.prepare(
        "SELECT id, source_path, source_hash, byte_start, byte_end, line_start, line_end, text, extractor, symbol
         FROM chunks
         WHERE symbol = ?1
         ORDER BY source_path ASC, byte_start ASC",
    )?;
    let rows = statement.query_map([symbol], chunk_record_from_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<ChunkRecord>>>()?)
}

/// Reads a four-column `SourceSpan` (byte_start, byte_end, line_start, line_end)
/// starting at `base`.
fn span_from_row(row: &rusqlite::Row<'_>, base: usize) -> rusqlite::Result<SourceSpan> {
    Ok(SourceSpan {
        byte_start: row_i64_to_usize(row, base)?,
        byte_end: row_i64_to_usize(row, base + 1)?,
        line_start: row_i64_to_usize(row, base + 2)?,
        line_end: row_i64_to_usize(row, base + 3)?,
    })
}

/// Re-validates one chunk against its source file on disk. The precise indexed
/// chunk is returned only when the file still hashes to the recorded
/// `source_hash`; a changed file is handed back in full (read live) so the
/// caller gets current code instead of a stale slice; a missing, unreadable, or
/// non-text source yields an `Unavailable` signal rather than untrustworthy
/// bytes.
fn validate_freshness(stored_root: &str, record: ChunkRecord) -> Retrieved {
    let Ok(canonical_root) = canonicalize_root(Path::new(stored_root)) else {
        // The corpus root directory is gone, so the source is too.
        return Retrieved::unavailable(&record, "source no longer exists");
    };

    let resolved = match canonicalize_discovered_file(
        canonical_root.as_path(),
        Path::new(&record.source_path),
    ) {
        Ok(path) => path,
        Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            return Retrieved::unavailable(&record, "source no longer exists");
        }
        // Any other resolution failure cannot produce trustworthy content.
        Err(_) => return Retrieved::unavailable(&record, "source is unreadable"),
    };

    // Defence in depth: a resolved file that escaped the corpus root is never
    // served.
    if canonical_relative_path(canonical_root.as_path(), resolved.as_path()).is_err() {
        return Retrieved::unavailable(&record, "source resolved outside the corpus");
    }

    let bytes = match read_corpus_file(resolved.as_path()) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Retrieved::unavailable(&record, "source no longer exists");
        }
        // Unreadable, no longer a regular file, or a swapped symlink.
        Err(_) => return Retrieved::unavailable(&record, "source is unreadable"),
    };

    let source_hash = blake3::hash(bytes.as_slice()).to_hex().to_string();
    if source_hash == record.source_hash {
        return Retrieved::Ok { chunk: record };
    }

    // Source changed since indexing. Hand back the whole current file so the
    // answer is real and up to date; refuse only if it is no longer UTF-8 text.
    match String::from_utf8(bytes) {
        Ok(text) => Retrieved::file(record.source_path.clone(), text),
        Err(_) => Retrieved::unavailable(&record, "source is no longer text"),
    }
}

/// Self-heals one source file in the bound index so a later query reads fresh
/// data. A changed file is re-chunked in place; a removed, escaped, or
/// non-text file has its chunks pruned; an unchanged file is left alone. Opens
/// the database read-write: callers reserve this for maintaining their own
/// launch-bound database, never an arbitrary path.
pub(crate) fn refresh_source(database: &Path, source_path: &str) -> Result<SourceRefresh> {
    let mut connection = open_database(database, Access::ReadWrite)?;

    let stored_root = connection
        .query_row(
            "SELECT value FROM metadata WHERE key = 'corpus_root'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or(Error::MissingCorpusRoot)?;
    let Ok(canonical_root) = canonicalize_root(Path::new(stored_root.as_str())) else {
        // The corpus root is gone; the source cannot be re-read, so drop it.
        return prune_source(&mut connection, source_path);
    };

    let resolved =
        match canonicalize_discovered_file(canonical_root.as_path(), Path::new(source_path)) {
            Ok(path) => path,
            Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                return prune_source(&mut connection, source_path);
            }
            Err(error) => return Err(error),
        };
    if canonical_relative_path(canonical_root.as_path(), resolved.as_path()).is_err() {
        return prune_source(&mut connection, source_path);
    }

    let bytes = match read_corpus_file(resolved.as_path()) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return prune_source(&mut connection, source_path);
        }
        Err(error) => return Err(Error::io(resolved, error)),
    };

    let source_hash = blake3::hash(bytes.as_slice()).to_hex().to_string();
    let existing_hash = connection
        .query_row(
            "SELECT content_hash FROM sources WHERE path = ?1",
            [source_path],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if existing_hash.as_deref() == Some(source_hash.as_str()) {
        return Ok(SourceRefresh::Unchanged);
    }

    let Ok(text) = String::from_utf8(bytes) else {
        // No longer indexable text; remove its stale chunks.
        return prune_source(&mut connection, source_path);
    };

    let extractor = crate::extract::extractor_id_for(source_path);
    let (chunks, _did_fallback) =
        crate::extract::extract_text(source_path, source_hash.as_str(), text.as_str());
    let transaction = connection.transaction()?;
    replace_source(
        &transaction,
        source_path,
        source_hash.as_str(),
        extractor,
        chunks.as_slice(),
    )?;
    transaction.commit()?;
    Ok(SourceRefresh::Reindexed {
        chunks: chunks.len(),
    })
}

/// Removes a source and its chunks. Deletes chunks explicitly so the FTS delete
/// trigger fires and the embedding rows cascade, then drops the source row.
fn prune_source(connection: &mut Connection, source_path: &str) -> Result<SourceRefresh> {
    let transaction = connection.transaction()?;
    transaction.execute("DELETE FROM chunks WHERE source_path = ?1", [source_path])?;
    transaction.execute("DELETE FROM sources WHERE path = ?1", [source_path])?;
    transaction.commit()?;
    Ok(SourceRefresh::Pruned)
}

/// Reports which of the given source paths have drifted from the index on disk:
/// the live file no longer hashes to the recorded `sources.content_hash`, is
/// gone, unreadable, escaped the root, or has no source row. Pure read; callers
/// use it to decide which files to self-heal before trusting headers. Order
/// follows the input, deduplicated.
pub(crate) fn drifted_sources(database: &Path, paths: &[String]) -> Result<Vec<String>> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }

    let connection = open_database(database, Access::ReadOnly)?;
    let stored_root = connection
        .query_row(
            "SELECT value FROM metadata WHERE key = 'corpus_root'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or(Error::MissingCorpusRoot)?;

    let mut seen = BTreeSet::new();
    let mut drifted = Vec::new();
    let canonical_root = canonicalize_root(Path::new(stored_root.as_str())).ok();
    for path in paths {
        if !seen.insert(path.as_str()) {
            continue;
        }
        let stored_hash = connection
            .query_row(
                "SELECT content_hash FROM sources WHERE path = ?1",
                [path],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(stored_hash) = stored_hash else {
            // No source row backs this header; treat it as drifted so the heal
            // pass prunes the orphan.
            drifted.push(path.clone());
            continue;
        };
        let still_fresh = canonical_root
            .as_deref()
            .is_some_and(|root| source_matches_disk(root, stored_hash.as_str(), path.as_str()));
        if !still_fresh {
            drifted.push(path.clone());
        }
    }

    Ok(drifted)
}

/// True when the live file at `source_path` still hashes to `stored_hash`. Any
/// missing, unreadable, escaped, or changed file reads as not matching.
fn source_matches_disk(canonical_root: &Path, stored_hash: &str, source_path: &str) -> bool {
    let Ok(resolved) = canonicalize_discovered_file(canonical_root, Path::new(source_path)) else {
        return false;
    };
    if canonical_relative_path(canonical_root, resolved.as_path()).is_err() {
        return false;
    }
    match read_corpus_file(resolved.as_path()) {
        Ok(bytes) => blake3::hash(bytes.as_slice()).to_hex().to_string() == stored_hash,
        Err(_) => false,
    }
}

pub(crate) fn corpus_root(database: &Path) -> Result<Option<PathBuf>> {
    let connection = open_database(database, Access::ReadOnly)?;
    let stored_root = connection
        .query_row(
            "SELECT value FROM metadata WHERE key = 'corpus_root'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    Ok(stored_root.map(PathBuf::from))
}

pub(crate) fn chunks_missing_embeddings(
    database: &Path,
    model_id: &str,
) -> Result<Vec<ChunkRecord>> {
    let connection = open_database(database, Access::ReadOnly)?;
    let mut statement = connection.prepare(
        "SELECT id, source_path, source_hash, byte_start, byte_end, line_start, line_end, text, extractor, symbol
         FROM chunks
         WHERE NOT EXISTS (
             SELECT 1
             FROM embeddings
             WHERE embeddings.chunk_id = chunks.id
               AND embeddings.model_id = ?1
         )
         ORDER BY source_path ASC, byte_start ASC, id ASC",
    )?;
    let rows = statement.query_map([model_id], chunk_record_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Error::from)
}

pub(crate) fn embedding_count(database: &Path, model_id: &str) -> Result<usize> {
    let connection = open_database(database, Access::ReadOnly)?;
    let count = connection.query_row(
        "SELECT COUNT(*) FROM embeddings WHERE model_id = ?1",
        [model_id],
        |row| row.get::<_, i64>(0),
    )?;
    usize::try_from(count).map_err(|source| {
        Error::Database(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Integer,
            Box::new(source),
        ))
    })
}

pub(crate) fn remove_other_model_embeddings(database: &Path, model_id: &str) -> Result<usize> {
    let connection = open_database(database, Access::ReadWrite)?;
    let removed = connection.execute("DELETE FROM embeddings WHERE model_id <> ?1", [model_id])?;
    Ok(removed)
}

pub(crate) fn upsert_embedding_batch(
    database: &Path,
    model_id: &str,
    dimensions: usize,
    batch: &[(String, Vec<f32>)],
) -> Result<()> {
    let mut connection = open_database(database, Access::ReadWrite)?;
    let transaction = connection.transaction()?;
    let mut statement = transaction.prepare(
        "INSERT INTO embeddings(chunk_id, model_id, dimensions, vector)
         VALUES(?1, ?2, ?3, ?4)
         ON CONFLICT(chunk_id, model_id) DO UPDATE SET
             dimensions = excluded.dimensions,
             vector = excluded.vector",
    )?;
    for (chunk_id, vector) in batch {
        statement.execute(params![
            chunk_id,
            model_id,
            usize_to_i64(dimensions)?,
            encode_vector(vector),
        ])?;
    }
    drop(statement);
    transaction.commit()?;
    Ok(())
}

pub(crate) fn embedding_storage_bytes(database: &Path, model_id: &str) -> Result<usize> {
    let connection = open_database(database, Access::ReadOnly)?;
    let bytes = connection.query_row(
        "SELECT COALESCE(SUM(length(vector)), 0)
         FROM embeddings
         WHERE model_id = ?1",
        [model_id],
        |row| row.get::<_, i64>(0),
    )?;
    usize::try_from(bytes).map_err(|source| {
        Error::Database(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Integer,
            Box::new(source),
        ))
    })
}

pub(crate) fn load_embedding_vectors(
    database: &Path,
    model_id: &str,
    dimensions: usize,
) -> Result<Vec<EmbeddingVector>> {
    let connection = open_database(database, Access::ReadOnly)?;
    let mut statement = connection.prepare(
        "SELECT chunks.id, chunks.source_path, chunks.byte_start, embeddings.vector
         FROM embeddings
         JOIN chunks ON chunks.id = embeddings.chunk_id
         WHERE embeddings.model_id = ?1
           AND embeddings.dimensions = ?2
         ORDER BY chunks.source_path ASC, chunks.byte_start ASC, chunks.id ASC",
    )?;
    let rows = statement.query_map(
        params![model_id, usize_to_i64(dimensions)?],
        |row| -> rusqlite::Result<EmbeddingVector> {
            let bytes = row.get::<_, Vec<u8>>(3)?;
            let vector = decode_vector(bytes.as_slice()).map_err(|message| {
                rusqlite::Error::FromSqlConversionFailure(
                    3,
                    rusqlite::types::Type::Blob,
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        message,
                    )),
                )
            })?;
            Ok(EmbeddingVector {
                chunk_id: row.get(0)?,
                source_path: row.get(1)?,
                byte_start: row_i64_to_usize(row, 2)?,
                vector,
            })
        },
    )?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Error::from)
}

/// Hydrates full chunk records for a set of identifiers, keyed by id. Batches
/// the lookup to stay under SQLite's bound-parameter limit so callers can pass
/// the full ranked candidate set.
pub(crate) fn chunks_by_ids(
    database: &Path,
    ids: &[String],
) -> Result<BTreeMap<String, ChunkRecord>> {
    let mut records = BTreeMap::new();
    if ids.is_empty() {
        return Ok(records);
    }

    let connection = open_database(database, Access::ReadOnly)?;
    for batch in ids.chunks(500) {
        let placeholders = vec!["?"; batch.len()].join(", ");
        let sql = format!(
            "SELECT id, source_path, source_hash, byte_start, byte_end, line_start, line_end, text, extractor, symbol
             FROM chunks
             WHERE id IN ({placeholders})"
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(rusqlite::params_from_iter(batch), chunk_record_from_row)?;
        for row in rows {
            let record = row?;
            records.insert(record.id.clone(), record);
        }
    }

    Ok(records)
}

pub(crate) fn corpus_db_stats(database: &Path) -> Result<crate::model::CorpusStatusReport> {
    let connection = open_database(database, Access::ReadOnly)?;

    let chunk_count: usize = {
        let n: i64 = connection.query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))?;
        usize::try_from(n).unwrap_or(0)
    };

    let embedding_model: Option<String> = connection
        .query_row(
            "SELECT model_id FROM embeddings GROUP BY model_id ORDER BY COUNT(*) DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;

    let embedding_count: usize = if let Some(model) = &embedding_model {
        let n: i64 = connection.query_row(
            "SELECT COUNT(*) FROM embeddings WHERE model_id = ?1",
            [model],
            |row| row.get(0),
        )?;
        usize::try_from(n).unwrap_or(0)
    } else {
        0
    };

    let symbol_count: usize = {
        let n: i64 = connection.query_row(
            "SELECT COUNT(*) FROM chunks WHERE symbol IS NOT NULL",
            [],
            |row| row.get(0),
        )?;
        usize::try_from(n).unwrap_or(0)
    };

    let edge_count: usize = {
        let n: i64 = connection.query_row("SELECT COUNT(*) FROM edges", [], |row| row.get(0))?;
        usize::try_from(n).unwrap_or(0)
    };

    let db_size_bytes = std::fs::metadata(database).map(|m| m.len()).unwrap_or(0);

    Ok(crate::model::CorpusStatusReport {
        chunk_count,
        embedding_count,
        embedding_model,
        db_size_bytes,
        symbol_count,
        edge_count,
    })
}

fn index_corpus_with_files<I, P, E, G>(
    canonical_root: &Path,
    database: &Path,
    files: I,
    initially_rejected: usize,
    extract_fn: E,
    extractor_id_fn: G,
    mode: IndexMode,
) -> Result<IndexReport>
where
    I: IntoIterator<Item = P>,
    P: Into<PathBuf>,
    E: Fn(&str, &str, &str) -> (Vec<Chunk>, bool),
    G: Fn(&str) -> &'static str,
{
    let mut connection = open_database(database, Access::Create)?;
    let canonical_root_string = normalize_path(canonical_root);
    ensure_corpus_root(&connection, canonical_root_string.as_str())?;

    let mut existing_sources = load_existing_sources(&connection)?;
    let mut discovered_paths = Vec::new();
    for file in files {
        discovered_paths.push(file.into());
    }

    let mut report = IndexReport {
        discovered: discovered_paths.len(),
        rejected: initially_rejected,
        ..IndexReport::default()
    };
    let mut seen_paths = BTreeSet::new();
    let transaction = connection.transaction()?;

    for discovered_path in discovered_paths {
        let relative_hint = relative_hint(canonical_root, discovered_path.as_path());

        let canonical_file =
            match canonicalize_discovered_file(canonical_root, discovered_path.as_path()) {
                Ok(path) => path,
                Err(Error::Io { .. }) => {
                    report.rejected += 1;
                    if let Some(path) = relative_hint {
                        if evict_source(&transaction, &mut existing_sources, path.as_str())? {
                            report.removed += 1;
                        }
                        seen_paths.insert(path);
                    }
                    continue;
                }
                Err(error) => return Err(error),
            };

        let relative_path = match canonical_relative_path(canonical_root, canonical_file.as_path())
        {
            Ok(path) => path,
            Err(_) => {
                report.rejected += 1;
                if let Some(path) = relative_hint {
                    if evict_source(&transaction, &mut existing_sources, path.as_str())? {
                        report.removed += 1;
                    }
                    seen_paths.insert(path);
                }
                continue;
            }
        };

        seen_paths.insert(relative_path.clone());

        let would_use_extractor = extractor_id_fn(relative_path.as_str());

        let bytes = match read_corpus_file(canonical_file.as_path()) {
            Ok(bytes) => bytes,
            Err(_) => {
                report.rejected += 1;
                if evict_source(&transaction, &mut existing_sources, relative_path.as_str())? {
                    report.removed += 1;
                }
                continue;
            }
        };
        let source_hash = blake3::hash(bytes.as_slice()).to_hex().to_string();

        if !mode.force
            && existing_sources.get(relative_path.as_str()).is_some_and(
                |(existing_hash, existing_extractor)| {
                    existing_hash == &source_hash && existing_extractor == would_use_extractor
                },
            )
        {
            report.unchanged += 1;
            continue;
        }

        let text = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => {
                report.rejected += 1;
                if evict_source(&transaction, &mut existing_sources, relative_path.as_str())? {
                    report.removed += 1;
                }
                continue;
            }
        };

        let (chunks, did_fallback) =
            extract_fn(relative_path.as_str(), source_hash.as_str(), text.as_str());
        replace_source(
            &transaction,
            relative_path.as_str(),
            source_hash.as_str(),
            would_use_extractor,
            chunks.as_slice(),
        )?;
        existing_sources.insert(relative_path, (source_hash, would_use_extractor.to_owned()));
        report.indexed += 1;
        report.chunks_written += chunks.len();
        if did_fallback {
            report.code_parse_fallbacks += 1;
        }
    }

    if mode.prune_missing {
        let stale_paths: Vec<String> = existing_sources
            .keys()
            .filter(|path| !seen_paths.contains(*path))
            .cloned()
            .collect();
        for stale_path in &stale_paths {
            transaction.execute("DELETE FROM sources WHERE path = ?1", [stale_path])?;
        }
        report.removed += stale_paths.len();
    }

    transaction.commit()?;
    Ok(report)
}

#[derive(Clone, Copy)]
enum Access {
    /// Open an existing database read-only. Never creates the file, so read
    /// commands cannot materialize a corpus on disk. An existing database whose
    /// schema is behind the current version is upgraded in place first (a
    /// schema-only migration of an existing corpus, not new content), so reads
    /// always see the current schema rather than failing on a missing column.
    ReadOnly,
    /// Open an existing database for writing, applying any pending migrations.
    ReadWrite,
    /// Create the database (and parent directory) if absent, then migrate.
    Create,
}

fn open_database(database: &Path, access: Access) -> Result<Connection> {
    if let Access::Create = access
        && let Some(parent) = database
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|source| Error::io(parent, source))?;
    }

    const READ_ONLY_FLAGS: OpenFlags = OpenFlags::SQLITE_OPEN_READ_ONLY
        .union(OpenFlags::SQLITE_OPEN_URI)
        .union(OpenFlags::SQLITE_OPEN_NO_MUTEX);

    let connection = match access {
        Access::ReadOnly => {
            let connection = Connection::open_with_flags(database, READ_ONLY_FLAGS)?;
            verify_schema_version(&connection)?;
            let user_version: i64 =
                connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
            if user_version < LATEST_SCHEMA_VERSION {
                // The corpus predates a schema-only migration. Upgrade it in
                // place through a write open, then continue read-only.
                drop(connection);
                drop(open_database(database, Access::ReadWrite)?);
                Connection::open_with_flags(database, READ_ONLY_FLAGS)?
            } else {
                connection
            }
        }
        Access::ReadWrite => {
            let mut connection = Connection::open_with_flags(
                database,
                OpenFlags::SQLITE_OPEN_READ_WRITE
                    | OpenFlags::SQLITE_OPEN_URI
                    | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            connection.pragma_update(None, "foreign_keys", 1_i64)?;
            apply_migrations(&mut connection)?;
            connection
        }
        Access::Create => {
            let mut connection = Connection::open(database)?;
            connection.pragma_update(None, "foreign_keys", 1_i64)?;
            apply_migrations(&mut connection)?;
            connection
        }
    };

    Ok(connection)
}

fn verify_schema_version(connection: &Connection) -> Result<()> {
    let user_version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if user_version > LATEST_SCHEMA_VERSION {
        return Err(Error::UnsupportedSchemaVersion {
            found: user_version,
            supported: LATEST_SCHEMA_VERSION,
        });
    }
    Ok(())
}

fn apply_migrations(connection: &mut Connection) -> Result<()> {
    verify_schema_version(connection)?;
    let mut user_version: i64 =
        connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;

    if user_version < 1 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(MIGRATION_001)?;
        transaction.pragma_update(None, "user_version", 1_i64)?;
        transaction.commit()?;
        user_version = 1;
    }

    if user_version < 2 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(MIGRATION_002)?;
        transaction.pragma_update(None, "user_version", 2_i64)?;
        transaction.commit()?;
        user_version = 2;
    }

    if user_version < 3 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(MIGRATION_003)?;
        transaction.pragma_update(None, "user_version", 3_i64)?;
        transaction.commit()?;
        user_version = 3;
    }

    if user_version < 4 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(MIGRATION_004)?;
        transaction.pragma_update(None, "user_version", 4_i64)?;
        transaction.commit()?;
        user_version = 4;
    }

    if user_version < 5 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(MIGRATION_005)?;
        transaction.pragma_update(None, "user_version", 5_i64)?;
        transaction.commit()?;
    }

    Ok(())
}

fn ensure_corpus_root(connection: &Connection, canonical_root: &str) -> Result<()> {
    let stored_root = connection
        .query_row(
            "SELECT value FROM metadata WHERE key = 'corpus_root'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;

    match stored_root {
        Some(stored_root) if stored_root != canonical_root => Err(Error::CorpusMismatch {
            expected: stored_root,
            actual: canonical_root.to_owned(),
        }),
        Some(_) => Ok(()),
        None => {
            connection.execute(
                "INSERT INTO metadata(key, value) VALUES('corpus_root', ?1)",
                [canonical_root],
            )?;
            Ok(())
        }
    }
}

fn load_existing_sources(connection: &Connection) -> Result<BTreeMap<String, (String, String)>> {
    let mut statement = connection
        .prepare("SELECT path, content_hash, extractor FROM sources ORDER BY path ASC")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;

    let mut sources = BTreeMap::new();
    for row in rows {
        let (path, content_hash, extractor) = row?;
        sources.insert(path, (content_hash, extractor));
    }

    Ok(sources)
}

fn replace_source(
    transaction: &rusqlite::Transaction<'_>,
    source_path: &str,
    source_hash: &str,
    extractor_id: &str,
    chunks: &[Chunk],
) -> Result<()> {
    transaction.execute(
        "INSERT INTO sources(path, content_hash, extractor)
         VALUES(?1, ?2, ?3)
         ON CONFLICT(path) DO UPDATE SET
             content_hash = excluded.content_hash,
             extractor = excluded.extractor",
        params![source_path, source_hash, extractor_id],
    )?;
    transaction.execute("DELETE FROM chunks WHERE source_path = ?1", [source_path])?;

    let mut statement = transaction.prepare(
        "INSERT INTO chunks(
            id,
            source_path,
            source_hash,
            byte_start,
            byte_end,
            line_start,
            line_end,
            text,
            extractor,
            symbol
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )?;

    // Edges are stored by callee name and cascade with their owning chunk: the
    // `DELETE FROM chunks` above already removed any prior edges for this source.
    let mut edge_statement = transaction.prepare(
        "INSERT INTO edges(
            src_chunk_id,
            edge_type,
            dst_symbol,
            confidence,
            byte_start,
            byte_end,
            line_start,
            line_end
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;

    for chunk in chunks {
        statement.execute(params![
            chunk.id.as_str(),
            chunk.source_path.as_str(),
            chunk.source_hash.as_str(),
            usize_to_i64(chunk.span.byte_start)?,
            usize_to_i64(chunk.span.byte_end)?,
            usize_to_i64(chunk.span.line_start)?,
            usize_to_i64(chunk.span.line_end)?,
            chunk.text.as_str(),
            chunk.extractor,
            chunk.symbol.as_deref(),
        ])?;

        for edge in &chunk.edges {
            edge_statement.execute(params![
                chunk.id.as_str(),
                edge.edge_type,
                edge.dst_symbol.as_str(),
                EDGE_CONFIDENCE_EXTRACTED,
                usize_to_i64(edge.span.byte_start)?,
                usize_to_i64(edge.span.byte_end)?,
                usize_to_i64(edge.span.line_start)?,
                usize_to_i64(edge.span.line_end)?,
            ])?;
        }
    }

    Ok(())
}

fn evict_source(
    transaction: &rusqlite::Transaction<'_>,
    existing_sources: &mut BTreeMap<String, (String, String)>,
    source_path: &str,
) -> Result<bool> {
    transaction.execute("DELETE FROM sources WHERE path = ?1", [source_path])?;
    Ok(existing_sources.remove(source_path).is_some())
}

fn chunk_record_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChunkRecord> {
    Ok(ChunkRecord {
        id: row.get(0)?,
        source_path: row.get(1)?,
        source_hash: row.get(2)?,
        span: SourceSpan {
            byte_start: row_i64_to_usize(row, 3)?,
            byte_end: row_i64_to_usize(row, 4)?,
            line_start: row_i64_to_usize(row, 5)?,
            line_end: row_i64_to_usize(row, 6)?,
        },
        text: row.get(7)?,
        extractor: row.get(8)?,
        symbol: row.get(9)?,
    })
}

fn row_i64_to_usize(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<usize> {
    let value = row.get::<_, i64>(index)?;
    usize::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
}

fn usize_to_i64(value: usize) -> Result<i64> {
    i64::try_from(value).map_err(|source| {
        Error::Database(rusqlite::Error::ToSqlConversionFailure(Box::new(source)))
    })
}

fn encode_vector(vector: &[f32]) -> Vec<u8> {
    vector
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn decode_vector(bytes: &[u8]) -> std::result::Result<Vec<f32>, &'static str> {
    if !bytes.len().is_multiple_of(std::mem::size_of::<f32>()) {
        return Err("embedding blob length is not a multiple of four");
    }
    Ok(bytes
        .chunks_exact(std::mem::size_of::<f32>())
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect())
}

fn canonicalize_root(root: &Path) -> Result<PathBuf> {
    let canonical_root =
        fs::canonicalize(root).map_err(|_| Error::InvalidRoot(root.to_path_buf()))?;
    if canonical_root.is_dir() {
        Ok(canonical_root)
    } else {
        Err(Error::InvalidRoot(root.to_path_buf()))
    }
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return fs::canonicalize(path).map_err(|source| Error::io(path, source));
    }

    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }

    std::env::current_dir()
        .map(|current| current.join(path))
        .map_err(|source| Error::io(path, source))
}

fn is_database_artifact(path: &Path, database: &Path) -> bool {
    if path == database {
        return true;
    }

    let database = database.as_os_str().to_string_lossy();
    let candidate = path.as_os_str().to_string_lossy();
    candidate == format!("{database}-wal") || candidate == format!("{database}-shm")
}

/// Reads a corpus file through a single descriptor, refusing to follow a final
/// symlink on platforms that support it. The file is validated and read through
/// the same handle, closing the window where a path checked during
/// canonicalization is swapped (for example, for a symlink out of the corpus)
/// before the read.
fn read_corpus_file(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read as _;

    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW);
    }

    let mut file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "corpus entry is not a regular file",
        ));
    }

    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn canonicalize_discovered_file(canonical_root: &Path, discovered_path: &Path) -> Result<PathBuf> {
    let candidate = if discovered_path.is_absolute() {
        discovered_path.to_path_buf()
    } else {
        canonical_root.join(discovered_path)
    };

    fs::canonicalize(candidate.as_path()).map_err(|source| Error::io(candidate, source))
}

fn canonical_relative_path(canonical_root: &Path, canonical_file: &Path) -> Result<String> {
    let relative = canonical_file
        .strip_prefix(canonical_root)
        .map_err(|_| Error::PathOutsideRoot(canonical_file.to_path_buf()))?;
    relative
        .to_str()
        .map(|path| path.replace('\\', "/"))
        .ok_or_else(|| Error::NonUtf8Path(relative.to_path_buf()))
}

fn relative_hint(canonical_root: &Path, discovered_path: &Path) -> Option<String> {
    let relative = if discovered_path.is_absolute() {
        discovered_path.strip_prefix(canonical_root).ok()?
    } else {
        discovered_path
    };
    relative.to_str().map(|path| path.replace('\\', "/"))
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn lexical_score(text: &str, lowercase_query: &str) -> f64 {
    let lowercase_text = text.to_lowercase();
    let mut count = 0.0;
    let mut remainder = lowercase_text.as_str();

    while let Some(index) = remainder.find(lowercase_query) {
        count += 1.0;
        remainder = &remainder[index + lowercase_query.len()..];
    }

    count
}

fn fts5_query(query: &str) -> Result<String> {
    let tokens = unique_tokens(tokenize(query).as_slice())?;

    Ok(tokens
        .into_iter()
        .map(|token| format!("\"{}\"", token.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" OR "))
}

fn tokenize(text: &str) -> Vec<String> {
    text.split(|character: char| !(character.is_alphanumeric() || character == '_'))
        .filter(|token| !token.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn unique_tokens(tokens: &[String]) -> Result<Vec<String>> {
    let mut unique = Vec::new();
    for token in tokens {
        if !unique.contains(token) {
            unique.push(token.clone());
        }
    }
    if unique.is_empty() {
        return Err(Error::NoSearchTerms);
    }
    Ok(unique)
}

fn contains_token_sequence(text: &[String], query: &[String]) -> bool {
    query.len() <= text.len() && text.windows(query.len()).any(|window| window == query)
}

#[cfg(test)]
mod tests {
    use super::*;

    use rusqlite::Connection;
    use tempfile::tempdir;

    #[cfg(unix)]
    #[test]
    fn read_corpus_file_reads_regular_files_and_refuses_symlinks() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().expect("tempdir");
        let target = temp.path().join("real.txt");
        fs::write(&target, "data").expect("write target");
        let link = temp.path().join("link.txt");
        symlink(&target, &link).expect("symlink");

        assert_eq!(
            read_corpus_file(target.as_path()).expect("read regular file"),
            b"data"
        );
        assert!(
            read_corpus_file(link.as_path()).is_err(),
            "a final-component symlink must be refused"
        );
    }

    fn write_text_file(path: &Path, text: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent directories");
        }
        fs::write(path, text).expect("write text file");
    }

    fn write_binary_file(path: &Path, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent directories");
        }
        fs::write(path, bytes).expect("write binary file");
    }

    fn list_relative_files(root: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        collect_relative_files(root, root, &mut files);
        files.sort();
        files
    }

    fn collect_relative_files(root: &Path, current: &Path, files: &mut Vec<PathBuf>) {
        let mut entries = fs::read_dir(current)
            .expect("read dir")
            .map(|entry| entry.expect("dir entry"))
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.path());

        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                collect_relative_files(root, path.as_path(), files);
            } else {
                files.push(
                    path.strip_prefix(root)
                        .expect("path beneath root")
                        .to_path_buf(),
                );
            }
        }
    }

    fn test_extract_text(relative_path: &str, source_hash: &str, text: &str) -> (Vec<Chunk>, bool) {
        let mut chunks = Vec::new();
        let mut cursor = 0usize;
        let mut line_start = 1usize;

        for segment in text.split("\n\n") {
            let byte_start = cursor;
            let byte_end = byte_start + segment.len();
            let line_count = segment.bytes().filter(|byte| *byte == b'\n').count() + 1;
            let line_end = line_start + line_count - 1;

            chunks.push(Chunk {
                id: format!("{relative_path}:{byte_start}:{byte_end}:{source_hash}"),
                source_path: relative_path.to_owned(),
                source_hash: source_hash.to_owned(),
                span: SourceSpan {
                    byte_start,
                    byte_end,
                    line_start,
                    line_end,
                },
                text: segment.to_owned(),
                extractor: "test-extractor",
                symbol: None,
                edges: Vec::new(),
            });

            cursor = byte_end + 2;
            line_start = line_end + 2;
        }

        (chunks, false)
    }

    fn index_fixture(root: &Path, database: &Path) -> Result<IndexReport> {
        let canonical_root = canonicalize_root(root)?;
        index_corpus_with_files(
            canonical_root.as_path(),
            database,
            list_relative_files(root),
            0,
            test_extract_text,
            |_| "test-extractor",
            IndexMode {
                force: false,
                prune_missing: true,
            },
        )
    }

    fn open_db(database: &Path) -> Connection {
        Connection::open(database).expect("open database")
    }

    fn ordered_chunk_ids(database: &Path) -> Vec<String> {
        let connection = open_database(database, Access::ReadOnly).expect("open database");
        let mut statement = connection
            .prepare("SELECT id FROM chunks ORDER BY source_path, byte_start, byte_end, id")
            .expect("prepare chunk id query");
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .expect("query chunk ids")
            .collect::<std::result::Result<Vec<_>, _>>()
            .expect("collect chunk ids")
    }

    #[test]
    fn indexes_first_corpus_and_creates_database_parent() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        write_text_file(root.join("docs/alpha.txt").as_path(), "alpha beta\n\nbeta");
        write_text_file(root.join("notes/gamma.txt").as_path(), "gamma");

        let database = temp.path().join("nested/db/mycelia.sqlite3");
        let report = index_fixture(root.as_path(), database.as_path()).expect("index corpus");

        assert_eq!(
            report,
            IndexReport {
                discovered: 2,
                indexed: 2,
                unchanged: 0,
                removed: 0,
                rejected: 0,
                chunks_written: 3,
                code_parse_fallbacks: 0,
                elapsed_ms: 0,
            }
        );

        let connection =
            open_database(database.as_path(), Access::ReadOnly).expect("open database");
        let user_version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("user version");
        let foreign_keys: i64 = connection
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .expect("foreign keys pragma");
        let corpus_root: String = connection
            .query_row(
                "SELECT value FROM metadata WHERE key = 'corpus_root'",
                [],
                |row| row.get(0),
            )
            .expect("metadata corpus_root");
        let source_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM sources", [], |row| row.get(0))
            .expect("source count");
        let chunk_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
            .expect("chunk count");

        assert_eq!(user_version, 5);
        assert_eq!(foreign_keys, 1);
        assert_eq!(
            corpus_root,
            normalize_path(&fs::canonicalize(root).expect("canonical root"))
        );
        assert_eq!(source_count, 2);
        assert_eq!(chunk_count, 3);
    }

    #[test]
    fn indexing_same_tree_twice_produces_identical_chunk_ids() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        write_text_file(
            root.join("src/lib.rs").as_path(),
            "/// Adds two values.\npub fn add(left: i32, right: i32) -> i32 {\n    left + right\n}\n",
        );
        write_text_file(
            root.join("src/review.ts").as_path(),
            "/** Reviews a value. */\nexport function review(value: string): string {\n  return value;\n}\n",
        );

        let first_database = temp.path().join("first.sqlite3");
        let second_database = temp.path().join("second.sqlite3");
        crate::index_corpus(root.as_path(), first_database.as_path()).expect("first index");
        crate::index_corpus(root.as_path(), second_database.as_path()).expect("second index");

        let first_ids = ordered_chunk_ids(first_database.as_path());
        let second_ids = ordered_chunk_ids(second_database.as_path());
        assert!(!first_ids.is_empty(), "fixture should produce chunks");
        assert_eq!(first_ids, second_ids);
    }

    #[test]
    fn read_open_upgrades_out_of_date_schema_in_place() {
        let temp = tempdir().expect("tempdir");
        let database = temp.path().join("legacy.sqlite3");

        // Build a genuine pre-graph (version 4) database: migrations 001..=004
        // only, so there is no `symbol` column and no `edges` table.
        {
            let connection = Connection::open(&database).expect("open");
            for migration in [MIGRATION_001, MIGRATION_002, MIGRATION_003, MIGRATION_004] {
                connection
                    .execute_batch(migration)
                    .expect("apply migration");
            }
            connection
                .pragma_update(None, "user_version", 4_i64)
                .expect("set version 4");
        }

        // A read open must transparently upgrade the schema rather than fail.
        let connection = open_database(&database, Access::ReadOnly).expect("read open upgrades");
        let user_version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("version");
        let edge_table: Option<String> = connection
            .query_row(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'edges'",
                [],
                |row| row.get(0),
            )
            .optional()
            .expect("edge table query");
        assert_eq!(user_version, 5);
        assert_eq!(edge_table.as_deref(), Some("edges"));
    }

    #[test]
    fn forced_reindex_backfills_graph_when_normal_index_would_skip() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        write_text_file(
            root.join("lib.rs").as_path(),
            "fn helper() -> i32 {\n    42\n}\n\nfn caller() -> i32 {\n    helper()\n}\n",
        );
        let database = temp.path().join("mycelia.sqlite3");
        index_corpus(root.as_path(), database.as_path()).expect("index");

        // Simulate a corpus indexed before the graph migration: strip its symbols
        // and edges, leaving content and source hashes untouched.
        {
            let connection = open_db(database.as_path());
            connection
                .execute("UPDATE chunks SET symbol = NULL", [])
                .expect("clear symbols");
            connection
                .execute("DELETE FROM edges", [])
                .expect("clear edges");
        }

        // A normal index skips the unchanged source, so the graph stays empty.
        index_corpus(root.as_path(), database.as_path()).expect("normal reindex");
        let after_normal: i64 = open_db(database.as_path())
            .query_row("SELECT COUNT(*) FROM edges", [], |row| row.get(0))
            .expect("edge count");
        assert_eq!(after_normal, 0);

        // A forced reindex re-extracts even the unchanged source and backfills.
        reindex_corpus(root.as_path(), database.as_path()).expect("forced reindex");
        let connection = open_db(database.as_path());
        let edges: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM edges WHERE dst_symbol = 'helper'",
                [],
                |row| row.get(0),
            )
            .expect("edge count");
        let symbols: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE symbol IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .expect("symbol count");
        assert_eq!(edges, 1);
        assert_eq!(symbols, 2);
    }

    #[test]
    fn rust_calls_edges_persist_and_cascade_on_reindex() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let file = root.join("lib.rs");
        write_text_file(
            file.as_path(),
            "fn helper() -> i32 { 42 }\n\nfn caller() -> i32 { helper() }\n",
        );
        let database = temp.path().join("mycelia.sqlite3");

        index_corpus(root.as_path(), database.as_path()).expect("index");

        let connection = open_db(database.as_path());
        let edge_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM edges WHERE dst_symbol = 'helper' AND edge_type = 'calls'",
                [],
                |row| row.get(0),
            )
            .expect("edge count");
        let symbol_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE symbol = 'caller'",
                [],
                |row| row.get(0),
            )
            .expect("symbol count");
        assert_eq!(edge_count, 1);
        assert_eq!(symbol_count, 1);
        drop(connection);

        // Rewriting the source to drop the call re-indexes the file, deleting its
        // chunks and cascading their edges; the helper edge must be gone.
        write_text_file(
            file.as_path(),
            "fn helper() -> i32 { 42 }\n\nfn caller() -> i32 { 0 }\n",
        );
        index_corpus(root.as_path(), database.as_path()).expect("reindex");

        let connection = open_db(database.as_path());
        let edge_count_after: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM edges WHERE dst_symbol = 'helper'",
                [],
                |row| row.get(0),
            )
            .expect("edge count after");
        assert_eq!(edge_count_after, 0);
    }

    #[test]
    fn find_relationships_resolves_unique_callers_and_callees() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        write_text_file(
            root.join("lib.rs").as_path(),
            "fn helper() -> i32 {\n    42\n}\n\n\
             fn caller() -> i32 {\n    helper()\n}\n\n\
             fn other() -> i32 {\n    helper()\n}\n",
        );
        let database = temp.path().join("mycelia.sqlite3");
        index_corpus(root.as_path(), database.as_path()).expect("index");

        let callers =
            find_relationships(database.as_path(), "helper", Direction::Callers).expect("callers");
        let caller_symbols: Vec<&str> = callers.iter().map(|hit| hit.symbol.as_str()).collect();
        assert_eq!(caller_symbols, vec!["caller", "other"]);
        assert!(
            callers
                .iter()
                .all(|hit| hit.resolved && hit.definition_count == 1)
        );
        // Each caller hit points the definition header at the calling chunk and
        // sources the call site inside it.
        assert!(callers[0].definition.source_path.ends_with("lib.rs"));

        let callees =
            find_relationships(database.as_path(), "caller", Direction::Callees).expect("callees");
        assert_eq!(callees.len(), 1);
        assert_eq!(callees[0].symbol, "helper");
        assert!(callees[0].resolved);
        assert_eq!(callees[0].definition.span.line_start, 1);
    }

    #[test]
    fn find_relationships_flags_ambiguity_and_omits_external() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        write_text_file(
            root.join("a.rs").as_path(),
            "fn helper() {}\n\nfn caller() {\n    helper();\n    println!(\"x\");\n}\n",
        );
        // A second definition of `helper` makes the name ambiguous.
        write_text_file(root.join("b.rs").as_path(), "fn helper() {}\n");
        let database = temp.path().join("mycelia.sqlite3");
        index_corpus(root.as_path(), database.as_path()).expect("index");

        // `helper` resolves to two definitions: every callee candidate is returned
        // and flagged, never collapsed to one.
        let callees =
            find_relationships(database.as_path(), "caller", Direction::Callees).expect("callees");
        let helper_hits: Vec<&RelatedHit> = callees
            .iter()
            .filter(|hit| hit.symbol == "helper")
            .collect();
        assert_eq!(helper_hits.len(), 2);
        assert!(
            helper_hits
                .iter()
                .all(|hit| !hit.resolved && hit.definition_count == 2)
        );
        // `println` has no in-corpus definition, so the external call is omitted.
        assert!(callees.iter().all(|hit| hit.symbol != "println"));

        let callers =
            find_relationships(database.as_path(), "helper", Direction::Callers).expect("callers");
        assert_eq!(callers.len(), 1);
        assert_eq!(callers[0].symbol, "caller");
        assert!(!callers[0].resolved);
        assert_eq!(callers[0].definition_count, 2);

        let external_callers =
            find_relationships(database.as_path(), "println", Direction::Callers)
                .expect("external callers");
        assert!(external_callers.is_empty());
    }

    #[test]
    fn rerun_skips_unchanged_sources() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        write_text_file(root.join("alpha.txt").as_path(), "alpha");
        let database = temp.path().join("mycelia.sqlite3");

        let first_report = index_fixture(root.as_path(), database.as_path()).expect("first index");
        let second_report =
            index_fixture(root.as_path(), database.as_path()).expect("second index");

        assert_eq!(first_report.indexed, 1);
        assert_eq!(
            second_report,
            IndexReport {
                discovered: 1,
                indexed: 0,
                unchanged: 1,
                removed: 0,
                rejected: 0,
                chunks_written: 0,
                code_parse_fallbacks: 0,
                elapsed_ms: 0,
            }
        );
    }

    #[test]
    fn database_inside_corpus_is_not_indexed() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        write_text_file(root.join("alpha.txt").as_path(), "alpha");
        let database = root.join(".mycelia/index.sqlite3");

        let first = index_corpus(root.as_path(), database.as_path()).expect("first index");
        let second = index_corpus(root.as_path(), database.as_path()).expect("second index");

        assert_eq!(first.discovered, 1);
        assert_eq!(first.indexed, 1);
        assert_eq!(first.rejected, 0);
        assert_eq!(second.discovered, 1);
        assert_eq!(second.unchanged, 1);
        assert_eq!(second.rejected, 0);
    }

    #[test]
    fn changed_file_replaces_prior_chunks_atomically() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let file = root.join("alpha.txt");
        let database = temp.path().join("mycelia.sqlite3");

        write_text_file(file.as_path(), "alpha\n\nbeta");
        index_fixture(root.as_path(), database.as_path()).expect("first index");

        write_text_file(file.as_path(), "alpha\n\nbeta beta");
        let report = index_fixture(root.as_path(), database.as_path()).expect("second index");

        assert_eq!(
            report,
            IndexReport {
                discovered: 1,
                indexed: 1,
                unchanged: 0,
                removed: 0,
                rejected: 0,
                chunks_written: 2,
                code_parse_fallbacks: 0,
                elapsed_ms: 0,
            }
        );

        let hits =
            find(database.as_path(), "beta", 10, RetrievalStrategy::Substring).expect("find hits");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].score, 2.0);
        assert_eq!(hits[0].chunk.text, "beta beta");
        let connection = open_db(database.as_path());
        let chunk_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
            .expect("chunk count");
        assert_eq!(chunk_count, 2);
    }

    #[test]
    fn removed_files_are_pruned_after_scan() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        write_text_file(root.join("alpha.txt").as_path(), "alpha");
        write_text_file(root.join("beta.txt").as_path(), "beta");
        let database = temp.path().join("mycelia.sqlite3");

        index_fixture(root.as_path(), database.as_path()).expect("first index");
        fs::remove_file(root.join("beta.txt")).expect("remove beta");

        let report = index_fixture(root.as_path(), database.as_path()).expect("second index");
        assert_eq!(
            report,
            IndexReport {
                discovered: 1,
                indexed: 0,
                unchanged: 1,
                removed: 1,
                rejected: 0,
                chunks_written: 0,
                code_parse_fallbacks: 0,
                elapsed_ms: 0,
            }
        );

        let connection = open_db(database.as_path());
        let source_paths = connection
            .prepare("SELECT path FROM sources ORDER BY path ASC")
            .expect("prepare select")
            .query_map([], |row| row.get::<_, String>(0))
            .expect("query source paths")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("collect source paths");
        assert_eq!(source_paths, vec!["alpha.txt".to_owned()]);
    }

    #[test]
    fn changed_source_refresh_does_not_prune_unchanged_sources() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        write_text_file(root.join("alpha.txt").as_path(), "alpha");
        write_text_file(root.join("beta.txt").as_path(), "beta");
        write_text_file(root.join("gamma.txt").as_path(), "gamma");
        let database = temp.path().join("mycelia.sqlite3");

        index_fixture(root.as_path(), database.as_path()).expect("first index");
        write_text_file(root.join("alpha.txt").as_path(), "alpha changed");
        fs::remove_file(root.join("beta.txt")).expect("remove beta");

        let report = refresh_changed_sources(
            root.as_path(),
            database.as_path(),
            ["alpha.txt", "beta.txt"],
        )
        .expect("changed refresh");
        assert_eq!(report.discovered, 2);
        assert_eq!(report.indexed, 1);
        assert_eq!(report.removed, 1);
        assert_eq!(report.rejected, 1);
        assert_eq!(report.chunks_written, 1);

        let hits = find(
            database.as_path(),
            "gamma",
            10,
            RetrievalStrategy::Substring,
        )
        .expect("find gamma");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].chunk.source_path, "gamma.txt");

        let removed =
            find(database.as_path(), "beta", 10, RetrievalStrategy::Substring).expect("find beta");
        assert!(removed.is_empty());
    }

    #[test]
    fn invalid_utf8_is_counted_as_rejected() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        write_binary_file(root.join("bad.txt").as_path(), &[0x66, 0x6f, 0x80, 0x6f]);
        let database = temp.path().join("mycelia.sqlite3");

        let report = index_fixture(root.as_path(), database.as_path()).expect("index corpus");
        assert_eq!(
            report,
            IndexReport {
                discovered: 1,
                indexed: 0,
                unchanged: 0,
                removed: 0,
                rejected: 1,
                chunks_written: 0,
                code_parse_fallbacks: 0,
                elapsed_ms: 0,
            }
        );
    }

    #[test]
    fn rejected_changed_source_evicts_stale_chunks() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let file = root.join("source.txt");
        let database = temp.path().join("mycelia.sqlite3");
        write_text_file(file.as_path(), "old searchable content");
        index_fixture(root.as_path(), database.as_path()).expect("first index");

        write_binary_file(file.as_path(), &[0xff, 0xfe]);
        let report = index_fixture(root.as_path(), database.as_path()).expect("second index");

        assert_eq!(report.rejected, 1);
        assert!(
            find(
                database.as_path(),
                "searchable",
                10,
                RetrievalStrategy::Substring,
            )
            .expect("find")
            .is_empty()
        );
    }

    #[test]
    fn rejects_root_mismatch_for_existing_database() {
        let temp = tempdir().expect("tempdir");
        let root_a = temp.path().join("root-a");
        let root_b = temp.path().join("root-b");
        write_text_file(root_a.join("alpha.txt").as_path(), "alpha");
        write_text_file(root_b.join("alpha.txt").as_path(), "alpha");
        let database = temp.path().join("mycelia.sqlite3");

        index_fixture(root_a.as_path(), database.as_path()).expect("index first root");
        let error = index_fixture(root_b.as_path(), database.as_path()).expect_err("root mismatch");

        match error {
            Error::CorpusMismatch { expected, actual } => {
                assert_eq!(
                    expected,
                    normalize_path(&fs::canonicalize(root_a).expect("canonical root a"))
                );
                assert_eq!(
                    actual,
                    normalize_path(&fs::canonicalize(root_b).expect("canonical root b"))
                );
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn find_orders_by_score_then_path_then_byte_start_then_id() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        write_text_file(root.join("a.txt").as_path(), "beta\n\nbeta");
        write_text_file(root.join("b.txt").as_path(), "beta beta beta");
        let database = temp.path().join("mycelia.sqlite3");

        index_fixture(root.as_path(), database.as_path()).expect("index corpus");
        let hits =
            find(database.as_path(), "BeTa", 10, RetrievalStrategy::Substring).expect("find hits");

        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].score, 3.0);
        assert_eq!(hits[0].chunk.source_path, "b.txt");
        assert_eq!(hits[1].score, 1.0);
        assert_eq!(hits[1].chunk.source_path, "a.txt");
        assert_eq!(hits[1].chunk.span.byte_start, 0);
        assert_eq!(hits[2].score, 1.0);
        assert_eq!(hits[2].chunk.source_path, "a.txt");
        assert!(hits[1].chunk.span.byte_start < hits[2].chunk.span.byte_start);
    }

    #[test]
    fn retrieve_returns_exact_chunk_with_provenance() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        write_text_file(root.join("a.txt").as_path(), "alpha\n\nbeta");
        let database = temp.path().join("mycelia.sqlite3");

        index_fixture(root.as_path(), database.as_path()).expect("index corpus");
        let hits =
            find(database.as_path(), "beta", 10, RetrievalStrategy::Substring).expect("find hits");
        let chunk_id = hits[0].chunk.id.clone();

        let outcome = retrieve(database.as_path(), chunk_id.as_str())
            .expect("retrieve result")
            .expect("chunk record");

        let Retrieved::Ok { chunk: record } = outcome else {
            panic!("expected a fresh chunk, got {outcome:?}");
        };
        assert_eq!(record.id, chunk_id);
        assert_eq!(record.source_path, "a.txt");
        assert_eq!(record.source_hash, hits[0].chunk.source_hash);
        assert_eq!(record.span.byte_start, hits[0].chunk.span.byte_start);
        assert_eq!(record.span.byte_end, hits[0].chunk.span.byte_end);
        assert_eq!(record.extractor, "test-extractor");
        assert_eq!(record.text, "beta");
    }

    #[test]
    fn retrieve_returns_whole_live_file_when_source_changes() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let file = root.join("a.txt");
        let database = temp.path().join("mycelia.sqlite3");
        write_text_file(file.as_path(), "alpha\n\nbeta");
        index_fixture(root.as_path(), database.as_path()).expect("index corpus");
        let chunk_id = find(database.as_path(), "beta", 10, RetrievalStrategy::Substring)
            .expect("find hits")[0]
            .chunk
            .id
            .clone();

        // Rewrite the source without re-indexing: the indexed `beta` chunk is now
        // stale, so retrieve must hand back the whole current file instead.
        write_text_file(file.as_path(), "alpha\n\ngamma\nadded line");
        let outcome = retrieve(database.as_path(), chunk_id.as_str())
            .expect("retrieve result")
            .expect("chunk present");

        match outcome {
            Retrieved::File {
                source_path,
                line_start,
                line_end,
                text,
            } => {
                assert_eq!(source_path, "a.txt");
                assert_eq!(line_start, 1);
                assert_eq!(line_end, 4);
                assert_eq!(text, "alpha\n\ngamma\nadded line");
            }
            other => panic!("expected whole-file fallback, got {other:?}"),
        }
    }

    #[test]
    fn retrieve_reports_unavailable_after_source_deleted() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let file = root.join("a.txt");
        let database = temp.path().join("mycelia.sqlite3");
        write_text_file(file.as_path(), "alpha\n\nbeta");
        index_fixture(root.as_path(), database.as_path()).expect("index corpus");
        let chunk_id = find(database.as_path(), "beta", 10, RetrievalStrategy::Substring)
            .expect("find hits")[0]
            .chunk
            .id
            .clone();

        fs::remove_file(file.as_path()).expect("remove source");
        let outcome = retrieve(database.as_path(), chunk_id.as_str())
            .expect("retrieve result")
            .expect("chunk present");

        match outcome {
            Retrieved::Unavailable {
                chunk_id: id,
                source_path,
                ..
            } => {
                assert_eq!(id, chunk_id);
                assert_eq!(source_path, "a.txt");
            }
            other => panic!("expected unavailable, got {other:?}"),
        }
    }

    #[test]
    fn refresh_source_reindexes_changed_file() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let file = root.join("a.txt");
        let database = temp.path().join("mycelia.sqlite3");
        write_text_file(file.as_path(), "alpha\n\nbeta");
        index_fixture(root.as_path(), database.as_path()).expect("index corpus");

        write_text_file(file.as_path(), "alpha\n\ngamma");
        let outcome = refresh_source(database.as_path(), "a.txt").expect("refresh changed source");

        assert!(matches!(outcome, SourceRefresh::Reindexed { chunks } if chunks == 2));
        // The index now reflects the new content: the old term is gone, the new
        // term is present and its chunk retrieves as fresh.
        assert!(
            find(database.as_path(), "beta", 10, RetrievalStrategy::Substring)
                .expect("find old")
                .is_empty()
        );
        let fresh = find(
            database.as_path(),
            "gamma",
            10,
            RetrievalStrategy::Substring,
        )
        .expect("find new");
        assert_eq!(fresh.len(), 1);
        let outcome = retrieve(database.as_path(), fresh[0].chunk.id.as_str())
            .expect("retrieve")
            .expect("present");
        assert!(matches!(outcome, Retrieved::Ok { .. }));
    }

    #[test]
    fn refresh_source_prunes_removed_file() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let file = root.join("a.txt");
        let database = temp.path().join("mycelia.sqlite3");
        write_text_file(file.as_path(), "alpha\n\nbeta");
        index_fixture(root.as_path(), database.as_path()).expect("index corpus");

        fs::remove_file(file.as_path()).expect("remove source");
        let outcome = refresh_source(database.as_path(), "a.txt").expect("refresh removed source");

        assert!(matches!(outcome, SourceRefresh::Pruned));
        assert!(
            find(database.as_path(), "beta", 10, RetrievalStrategy::Fts5)
                .expect("find pruned")
                .is_empty()
        );
        let connection = open_db(database.as_path());
        let source_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM sources", [], |row| row.get(0))
            .expect("source count");
        assert_eq!(source_count, 0);
    }

    #[test]
    fn drifted_sources_reports_only_changed_or_missing_paths() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let changed = root.join("changed.txt");
        let removed = root.join("removed.txt");
        write_text_file(changed.as_path(), "first");
        write_text_file(removed.as_path(), "second");
        write_text_file(root.join("stable.txt").as_path(), "third");
        let database = temp.path().join("mycelia.sqlite3");
        index_fixture(root.as_path(), database.as_path()).expect("index corpus");

        write_text_file(changed.as_path(), "first edited");
        fs::remove_file(removed.as_path()).expect("remove source");

        let drifted = drifted_sources(
            database.as_path(),
            &[
                "changed.txt".to_owned(),
                "stable.txt".to_owned(),
                "removed.txt".to_owned(),
                // A duplicate and an unbacked path: deduplicated, unbacked drifts.
                "changed.txt".to_owned(),
                "never-indexed.txt".to_owned(),
            ],
        )
        .expect("drift scan");

        assert_eq!(
            drifted,
            vec![
                "changed.txt".to_owned(),
                "removed.txt".to_owned(),
                "never-indexed.txt".to_owned(),
            ]
        );
    }

    #[test]
    fn refresh_source_leaves_unchanged_file_alone() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let database = temp.path().join("mycelia.sqlite3");
        write_text_file(root.join("a.txt").as_path(), "alpha\n\nbeta");
        index_fixture(root.as_path(), database.as_path()).expect("index corpus");

        let outcome =
            refresh_source(database.as_path(), "a.txt").expect("refresh unchanged source");
        assert!(matches!(outcome, SourceRefresh::Unchanged));
    }

    #[test]
    fn retrieve_returns_none_for_unknown_chunk() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let database = temp.path().join("mycelia.sqlite3");
        write_text_file(root.join("a.txt").as_path(), "alpha");
        index_fixture(root.as_path(), database.as_path()).expect("index corpus");

        assert!(
            retrieve(database.as_path(), "missing")
                .expect("retrieve result")
                .is_none()
        );
    }

    #[test]
    fn validates_arguments() {
        let temp = tempdir().expect("tempdir");
        let database = temp.path().join("mycelia.sqlite3");
        let missing_root = temp.path().join("missing");

        assert!(matches!(
            canonicalize_root(missing_root.as_path()),
            Err(Error::InvalidRoot(path)) if path == missing_root
        ));
        assert!(matches!(
            find(database.as_path(), "   ", 1, RetrievalStrategy::Substring),
            Err(Error::EmptyQuery)
        ));
        assert!(matches!(
            find(database.as_path(), "query", 0, RetrievalStrategy::Substring),
            Err(Error::InvalidLimit)
        ));
    }

    #[test]
    fn rejects_newer_database_schema() {
        let temp = tempdir().expect("tempdir");
        let database = temp.path().join("future.sqlite3");
        let connection = Connection::open(database.as_path()).expect("open database");
        connection
            .pragma_update(None, "user_version", 99_i64)
            .expect("set user version");
        drop(connection);

        assert!(matches!(
            open_database(database.as_path(), Access::ReadOnly),
            Err(Error::UnsupportedSchemaVersion {
                found: 99,
                supported: 5
            })
        ));
    }

    #[test]
    fn fts5_matches_reordered_terms_and_punctuation() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let database = temp.path().join("mycelia.sqlite3");
        write_text_file(
            root.join("architecture.md").as_path(),
            "Rust core keeps the web UI local.",
        );
        write_text_file(root.join("other.md").as_path(), "Rust appears alone.");
        index_fixture(root.as_path(), database.as_path()).expect("index corpus");

        let hits = find(
            database.as_path(),
            "Tauri: web UI, Rust core",
            10,
            RetrievalStrategy::Fts5,
        )
        .expect("find hits");

        assert_eq!(hits[0].chunk.source_path, "architecture.md");
        assert!(hits[0].score.is_finite());
    }

    #[test]
    fn reranker_rewards_normalized_phrase_and_token_coverage() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let database = temp.path().join("mycelia.sqlite3");
        write_text_file(
            root.join("target.md").as_path(),
            "Use the official **subscription** client for deep work.",
        );
        for index in 0..20 {
            write_text_file(
                root.join(format!("noise-{index}.md")).as_path(),
                "Official client guidance with unrelated content.",
            );
        }
        index_fixture(root.as_path(), database.as_path()).expect("index corpus");

        let hits = find(
            database.as_path(),
            "official subscription client",
            5,
            RetrievalStrategy::Fts5Reranked,
        )
        .expect("reranked hits");

        assert_eq!(hits[0].chunk.source_path, "target.md");
    }

    #[test]
    fn reranker_lifts_symbol_definitions_above_references() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let database = temp.path().join("mycelia.sqlite3");
        // The definition carries the symbol in its leading signature line.
        write_text_file(
            root.join("def.txt").as_path(),
            "RunStore definition\nthis type holds runs.",
        );
        // The reference repeats the symbol in its body (favouring raw BM25) but
        // never in the leading line, so signature coverage should outrank it.
        write_text_file(
            root.join("ref.txt").as_path(),
            "helper usage notes\ncall RunStore then RunStore and RunStore again.",
        );
        index_fixture(root.as_path(), database.as_path()).expect("index corpus");

        let hits = find(
            database.as_path(),
            "RunStore",
            5,
            RetrievalStrategy::Fts5Reranked,
        )
        .expect("reranked hits");

        assert_eq!(hits[0].chunk.source_path, "def.txt");
    }

    #[test]
    fn reranker_ignores_signature_line_for_prose_terms() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let database = temp.path().join("mycelia.sqlite3");
        // A plain prose word in a leading line must not earn a signature boost:
        // only identifier-shaped query terms feed that signal. The chunk with
        // stronger body coverage should still win.
        write_text_file(
            root.join("header.txt").as_path(),
            "storage notes\nunrelated body text here.",
        );
        write_text_file(
            root.join("body.txt").as_path(),
            "general notes\nthe storage layer persists storage records to storage.",
        );
        index_fixture(root.as_path(), database.as_path()).expect("index corpus");

        let hits = find(
            database.as_path(),
            "storage",
            5,
            RetrievalStrategy::Fts5Reranked,
        )
        .expect("reranked hits");

        assert_eq!(hits[0].chunk.source_path, "body.txt");
    }

    #[test]
    fn reranker_deduplicates_identical_chunk_texts() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let database = temp.path().join("mycelia.sqlite3");
        for name in ["a.txt", "b.txt", "c.txt"] {
            write_text_file(
                root.join(name).as_path(),
                "workspace child contract repeated boilerplate",
            );
        }
        write_text_file(
            root.join("z.txt").as_path(),
            "workspace child contract specific answer",
        );
        index_fixture(root.as_path(), database.as_path()).expect("index corpus");

        let hits = find(
            database.as_path(),
            "workspace child contract",
            2,
            RetrievalStrategy::Fts5Reranked,
        )
        .expect("reranked hits");

        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].chunk.source_path, "a.txt");
        assert_eq!(hits[1].chunk.source_path, "z.txt");
    }

    #[test]
    fn fts5_tracks_changed_and_removed_chunks() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let file = root.join("source.txt");
        let database = temp.path().join("mycelia.sqlite3");
        write_text_file(file.as_path(), "old token");
        index_fixture(root.as_path(), database.as_path()).expect("first index");

        write_text_file(file.as_path(), "new token");
        index_fixture(root.as_path(), database.as_path()).expect("changed index");
        assert!(
            find(database.as_path(), "old", 10, RetrievalStrategy::Fts5)
                .expect("old query")
                .is_empty()
        );
        assert_eq!(
            find(database.as_path(), "new", 10, RetrievalStrategy::Fts5)
                .expect("new query")
                .len(),
            1
        );

        fs::remove_file(file).expect("remove source");
        index_fixture(root.as_path(), database.as_path()).expect("removed index");
        assert!(
            find(database.as_path(), "new", 10, RetrievalStrategy::Fts5)
                .expect("removed query")
                .is_empty()
        );
    }

    #[test]
    fn migration_two_backfills_existing_chunks() {
        let temp = tempdir().expect("tempdir");
        let database = temp.path().join("version-one.sqlite3");
        let connection = Connection::open(database.as_path()).expect("open database");
        connection
            .execute_batch(MIGRATION_001)
            .expect("apply migration one");
        connection
            .execute(
                "INSERT INTO sources(path, content_hash) VALUES('legacy.txt', 'hash')",
                [],
            )
            .expect("insert source");
        connection
            .execute(
                "INSERT INTO chunks(
                    id, source_path, source_hash, byte_start, byte_end,
                    line_start, line_end, text, extractor
                 ) VALUES(
                    'legacy', 'legacy.txt', 'hash', 0, 12, 1, 1,
                    'legacy token', 'test'
                 )",
                [],
            )
            .expect("insert chunk");
        connection
            .pragma_update(None, "user_version", 1_i64)
            .expect("set version one");
        drop(connection);

        // Migrations run on write paths, not on read commands, so open the
        // legacy database read-write to migrate it before searching.
        open_database(database.as_path(), Access::ReadWrite).expect("migrate database");
        let hits = find(database.as_path(), "legacy", 10, RetrievalStrategy::Fts5)
            .expect("migrated search");

        assert_eq!(hits.len(), 1);
        let connection = Connection::open(database).expect("reopen database");
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("schema version");
        let embedding_table: String = connection
            .query_row(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'embeddings'",
                [],
                |row| row.get(0),
            )
            .expect("embedding table");
        assert_eq!(version, 5);
        assert_eq!(embedding_table, "embeddings");
    }

    #[test]
    fn fts5_rejects_punctuation_only_queries() {
        let temp = tempdir().expect("tempdir");
        let database = temp.path().join("index.sqlite3");

        assert!(matches!(
            find(database.as_path(), "---", 10, RetrievalStrategy::Fts5),
            Err(Error::NoSearchTerms)
        ));
    }

    // blast_radius: changed-path chunks + cross-file callers / callees
    #[test]
    fn blast_radius_returns_changed_and_related_chunks() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        // helper.rs defines `helper` (changed file) and `stable`.
        write_text_file(
            root.join("helper.rs").as_path(),
            "pub fn helper() -> i32 { 42 }\n\npub fn stable() -> bool { true }\n",
        );
        // caller.rs is NOT in changed_paths — it calls `helper` from another file.
        write_text_file(
            root.join("caller.rs").as_path(),
            "fn run() -> i32 { helper() }\n",
        );
        let database = temp.path().join("mycelia.sqlite3");
        index_corpus(root.as_path(), database.as_path()).expect("index");

        let changed_paths = vec!["helper.rs".to_owned()];
        let result = blast_radius(database.as_path(), &changed_paths, 20).expect("blast_radius");

        // Changed-path chunks: `helper` and `stable`.
        let changed: Vec<&str> = result
            .iter()
            .filter(|h| h.source_path == "helper.rs")
            .filter_map(|h| h.signature.as_deref())
            .collect();
        assert!(
            changed.iter().any(|s| s.contains("helper")),
            "expected helper.rs chunks in result; got {changed:?}"
        );
        // Caller chunk: `run` in caller.rs calls `helper`.
        let caller_paths: Vec<&str> = result.iter().map(|h| h.source_path.as_str()).collect();
        assert!(
            caller_paths.contains(&"caller.rs"),
            "expected caller.rs in blast radius; got {caller_paths:?}"
        );
        // Changed-path chunks score 1.0; blast-radius chunks score 0.5.
        let helper_score = result
            .iter()
            .find(|h| {
                h.source_path == "helper.rs"
                    && h.signature
                        .as_deref()
                        .map_or(false, |s| s.contains("fn helper"))
            })
            .map(|h| h.score)
            .expect("helper chunk");
        assert_eq!(helper_score, 1.0, "changed-path chunk must score 1.0");
        let caller_score = result
            .iter()
            .find(|h| h.source_path == "caller.rs")
            .map(|h| h.score)
            .expect("caller chunk");
        assert_eq!(caller_score, 0.5, "blast-radius chunk must score 0.5");
    }

    #[test]
    fn blast_radius_returns_empty_for_empty_paths() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        write_text_file(root.join("lib.rs").as_path(), "fn foo() {}\n");
        let database = temp.path().join("mycelia.sqlite3");
        index_corpus(root.as_path(), database.as_path()).expect("index");
        let result = blast_radius(database.as_path(), &[], 10).expect("blast_radius empty");
        assert!(result.is_empty());
    }
}
