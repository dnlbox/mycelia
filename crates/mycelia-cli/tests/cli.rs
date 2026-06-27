use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use tempfile::TempDir;

fn bin_path() -> &'static str {
    env!("CARGO_BIN_EXE_mycelia")
}

fn write_file(path: &Path, contents: &str) {
    fs::write(path, contents).expect("write fixture file");
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(bin_path())
        .args(args)
        .output()
        .expect("run mycelia")
}

fn run_success(args: &[&str]) -> String {
    let output = run(args);
    successful_stdout(output)
}

fn run_with_homes(args: &[&str], config_home: &Path, data_home: &Path) -> std::process::Output {
    Command::new(bin_path())
        .args(args)
        .env("MYCELIA_CONFIG_HOME", config_home)
        .env("MYCELIA_DATA_HOME", data_home)
        .output()
        .expect("run mycelia with isolated homes")
}

fn run_success_with_homes(args: &[&str], config_home: &Path, data_home: &Path) -> String {
    successful_stdout(run_with_homes(args, config_home, data_home))
}

fn successful_stdout(output: std::process::Output) -> String {
    assert!(
        output.status.success(),
        "command failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("utf8 stdout")
}

fn write_json_line(writer: &mut impl Write, value: &Value) {
    serde_json::to_writer(&mut *writer, value).expect("write json");
    writer.write_all(b"\n").expect("write newline");
    writer.flush().expect("flush json");
}

fn read_json_line(reader: &mut impl BufRead) -> Value {
    let mut line = String::new();
    reader.read_line(&mut line).expect("read json line");
    assert!(!line.is_empty(), "MCP server closed before responding");
    serde_json::from_str(&line).expect("parse MCP response")
}

#[test]
fn version_flag_exits_zero_on_stdout() {
    let output = run(["--version"].as_slice());
    assert!(
        output.status.success(),
        "version failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(
        stdout.starts_with("mycelia "),
        "unexpected version output: {stdout}"
    );
}

#[test]
fn retired_corpus_command_points_to_new_verbs() {
    let output = run(["corpus"].as_slice());
    assert!(
        !output.status.success(),
        "retired corpus command should fail"
    );
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(
        stderr.contains("Use `mycelia setup`, `mycelia status`, or `mycelia list`"),
        "missing migration hint:\n{stderr}"
    );
}

#[test]
fn routed_find_without_embeddings_falls_back_to_lexical() {
    let temp = TempDir::new().expect("tempdir");
    let root = temp.path().join("corpus");
    let database = temp.path().join("mycelia.sqlite");
    fs::create_dir_all(&root).expect("create corpus");
    write_file(&root.join("notes.txt"), "hello routed world\n");

    run_success(&[
        "index",
        root.to_str().expect("root path"),
        "--database",
        database.to_str().expect("database path"),
    ]);

    // The default strategy is routed. With no embeddings present, it must
    // answer via reranked FTS5 without initializing (or downloading) the
    // embedding model, keeping the default query path cheap and offline.
    let find_stdout = run_success(&[
        "find",
        "hello",
        "--database",
        database.to_str().expect("database path"),
        "--json",
    ]);
    let hits: Value = serde_json::from_str(&find_stdout).expect("parse find json");
    assert!(
        hits.as_array().is_some_and(|items| !items.is_empty()),
        "routed fallback should return hits:\n{find_stdout}"
    );
    assert!(
        !database
            .parent()
            .expect("database parent")
            .join("models")
            .exists(),
        "routed fallback must not initialize the embedding model cache"
    );
}

#[test]
fn index_find_and_retrieve_work_end_to_end() {
    let temp = TempDir::new().expect("tempdir");
    let root = temp.path().join("corpus");
    let database = temp.path().join("mycelia.sqlite");
    fs::create_dir_all(&root).expect("create corpus");
    write_file(
        &root.join("notes.txt"),
        "hello world\n\nthis chunk should be retrievable\n",
    );

    let index_stdout = run_success(&[
        "index",
        root.to_str().expect("root path"),
        "--database",
        database.to_str().expect("database path"),
    ]);
    for expected in [
        "discovered:",
        "indexed:",
        "unchanged:",
        "removed:",
        "rejected:",
        "chunks_written:",
        "elapsed_ms:",
    ] {
        assert!(
            index_stdout.contains(expected),
            "missing `{expected}` in index output:\n{index_stdout}"
        );
    }

    let find_stdout = run_success(&[
        "find",
        "hello",
        "--database",
        database.to_str().expect("database path"),
        "--strategy",
        "fts5-reranked",
        "--json",
    ]);
    let hits: Value = serde_json::from_str(&find_stdout).expect("parse find json");
    let first_hit = hits
        .as_array()
        .and_then(|items| items.first())
        .expect("at least one search hit");
    let chunk_id = first_hit["chunk_id"]
        .as_str()
        .expect("chunk id")
        .to_string();
    assert!(
        first_hit.get("text").is_none(),
        "find should return headers without chunk bodies"
    );

    let retrieve_stdout = run_success(&[
        "retrieve",
        &chunk_id,
        "--database",
        database.to_str().expect("database path"),
    ]);
    for expected in [
        format!("id: {chunk_id}"),
        "path:".to_owned(),
        "byte range:".to_owned(),
        "line range:".to_owned(),
        "extractor:".to_owned(),
        "text:".to_owned(),
    ] {
        assert!(
            retrieve_stdout.contains(&expected),
            "missing `{expected}` in retrieve output:\n{retrieve_stdout}"
        );
    }
    assert!(
        !retrieve_stdout.contains("score:"),
        "retrieve output should not include a score:\n{retrieve_stdout}"
    );
}

#[test]
fn retrieve_without_chunk_id_returns_clear_error() {
    let temp = TempDir::new().expect("tempdir");
    let database = temp.path().join("mycelia.sqlite");
    let output = run(&[
        "retrieve",
        "--database",
        database.to_str().expect("database path"),
    ]);

    assert!(
        !output.status.success(),
        "retrieve should fail without chunk id"
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("error: retrieve requires a chunk id"),
        "stderr did not contain the expected error:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn evaluates_manifest_against_existing_index() {
    let temp = TempDir::new().expect("tempdir");
    let root = temp.path().join("corpus");
    let database = temp.path().join("mycelia.sqlite");
    let manifest = temp.path().join("evaluation.json");
    fs::create_dir_all(&root).expect("create corpus");
    write_file(&root.join("notes.txt"), "a precise sourced result");
    write_file(
        &manifest,
        r#"{
          "limit": 5,
          "cases": [{
            "name": "sourced result",
            "query": "precise",
            "expected": [{
              "source_path": "notes.txt",
              "contains": "sourced result"
            }]
          }]
        }"#,
    );

    run_success(&[
        "index",
        root.to_str().expect("root path"),
        "--database",
        database.to_str().expect("database path"),
    ]);
    // Pin the lexical strategy so this exercises eval mechanics without loading
    // the embedding model (the default is now `routed`).
    let output = run_success(&[
        "eval",
        manifest.to_str().expect("manifest path"),
        "--database",
        database.to_str().expect("database path"),
        "--strategy",
        "fts5-reranked",
        "--json",
    ]);
    let report: Value = serde_json::from_str(&output).expect("parse evaluation report");

    assert_eq!(report["cases"], 1);
    assert_eq!(report["hits"], 1);
    assert_eq!(report["strategy"], "fts5_reranked");
    assert_eq!(report["hit_rate"], 1.0);
    assert_eq!(report["mean_reciprocal_rank"], 1.0);
    assert_eq!(report["results"][0]["rank"], 1);
}

#[test]
fn fts5_strategy_matches_reordered_terms() {
    let temp = TempDir::new().expect("tempdir");
    let root = temp.path().join("corpus");
    let database = temp.path().join("mycelia.sqlite");
    fs::create_dir_all(&root).expect("create corpus");
    write_file(&root.join("architecture.md"), "Rust core with a web UI");

    run_success(&[
        "index",
        root.to_str().expect("root path"),
        "--database",
        database.to_str().expect("database path"),
    ]);
    let output = run_success(&[
        "find",
        "web UI Rust",
        "--database",
        database.to_str().expect("database path"),
        "--strategy",
        "fts5",
        "--json",
    ]);
    let hits: Value = serde_json::from_str(&output).expect("parse hits");

    assert_eq!(hits[0]["source_path"], "architecture.md");
    assert!(hits[0].get("text").is_none());
}

#[test]
fn named_corpus_profile_indexes_and_queries_derived_database() {
    let temp = TempDir::new().expect("tempdir");
    let root = temp.path().join("corpus");
    let config_home = temp.path().join("config");
    let data_home = temp.path().join("data");
    fs::create_dir_all(&root).expect("create corpus");
    write_file(&root.join("notes.txt"), "a precise named corpus result");

    // `setup --no-embed` registers the profile, indexes, and skips embedding so
    // the test stays offline without downloading the model.
    let setup_stderr = {
        let out = Command::new(bin_path())
            .args([
                "setup",
                root.to_str().expect("root path"),
                "--name",
                "forge",
                "--no-embed",
            ])
            .env("MYCELIA_CONFIG_HOME", &config_home)
            .env("MYCELIA_DATA_HOME", &data_home)
            .output()
            .expect("run setup");
        assert!(
            out.status.success(),
            "setup failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stderr).into_owned()
    };
    assert!(
        setup_stderr.contains("forge"),
        "setup should mention corpus name"
    );
    assert!(data_home.join("corpora/forge.sqlite3").is_file());

    // `list` should show the registered corpus.
    let list_output = run_success_with_homes(&["list"], &config_home, &data_home);
    assert!(list_output.contains("forge"));

    // `find --corpus` routes through the named profile.
    let find_output = run_success_with_homes(
        &[
            "find",
            "precise",
            "--corpus",
            "forge",
            "--strategy",
            "fts5-reranked",
            "--json",
        ],
        &config_home,
        &data_home,
    );
    let hits: Value = serde_json::from_str(&find_output).expect("parse profile hits");
    assert_eq!(hits[0]["source_path"], "notes.txt");
    assert!(hits[0].get("text").is_none());
}

#[test]
fn connect_codex_writes_idempotent_mcp_server_config() {
    let temp = TempDir::new().expect("tempdir");
    let root = temp.path().join("corpus");
    let config_home = temp.path().join("config");
    let data_home = temp.path().join("data");
    let home = temp.path().join("home");
    fs::create_dir_all(&root).expect("create corpus");
    fs::create_dir_all(home.join(".codex")).expect("create codex config dir");
    fs::write(home.join(".codex/config.toml"), "model = \"test\"\n").expect("seed config");
    write_file(&root.join("notes.txt"), "a precise named corpus result");

    let setup = Command::new(bin_path())
        .args([
            "setup",
            root.to_str().expect("root path"),
            "--name",
            "forge",
            "--no-embed",
        ])
        .env("MYCELIA_CONFIG_HOME", &config_home)
        .env("MYCELIA_DATA_HOME", &data_home)
        .env("HOME", &home)
        .output()
        .expect("run setup");
    assert!(
        setup.status.success(),
        "setup failed:\n{}",
        String::from_utf8_lossy(&setup.stderr)
    );

    for _ in 0..2 {
        let output = Command::new(bin_path())
            .args(["connect", "codex", "--corpus", "forge"])
            .env("MYCELIA_CONFIG_HOME", &config_home)
            .env("MYCELIA_DATA_HOME", &data_home)
            .env("HOME", &home)
            .output()
            .expect("run connect");
        assert!(
            output.status.success(),
            "connect failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let config = fs::read_to_string(home.join(".codex/config.toml")).expect("read config");
    assert!(config.contains("model = \"test\""));
    assert_eq!(config.matches("[mcp_servers.mycelia-forge]").count(), 1);
    assert!(config.contains("command = "));
    assert!(config.contains("args = [\"serve\", \"--corpus\", \"forge\"]"));
}

#[test]
fn corpus_profiles_reject_unsafe_names_and_mixed_targets() {
    let temp = TempDir::new().expect("tempdir");
    let root = temp.path().join("corpus");
    let database = temp.path().join("explicit.sqlite3");
    let config_home = temp.path().join("config");
    let data_home = temp.path().join("data");
    fs::create_dir_all(&root).expect("create corpus");

    let invalid = run_with_homes(
        &[
            "setup",
            root.to_str().expect("root path"),
            "--name",
            "../forge",
            "--no-embed",
        ],
        &config_home,
        &data_home,
    );
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("invalid corpus name"));

    let mixed = run_with_homes(
        &[
            "find",
            "query",
            "--corpus",
            "forge",
            "--database",
            database.to_str().expect("database path"),
        ],
        &config_home,
        &data_home,
    );
    assert!(!mixed.status.success());
    assert!(String::from_utf8_lossy(&mixed.stderr).contains("cannot be used with"));
}

#[test]
fn stdio_mcp_uses_named_corpus_and_calls_read_only_tools() {
    let temp = TempDir::new().expect("tempdir");
    let root = temp.path().join("corpus");
    let config_home = temp.path().join("config");
    let data_home = temp.path().join("data");
    fs::create_dir_all(&root).expect("create corpus");
    write_file(&root.join("notes.txt"), "a precise sourced result");
    run_success_with_homes(
        &[
            "setup",
            root.to_str().expect("root path"),
            "--name",
            "forge",
            "--no-embed",
        ],
        &config_home,
        &data_home,
    );

    let mut child = Command::new(bin_path())
        // Lexical mode keeps the test offline; routing is covered by core tests.
        .args(["serve", "--corpus", "forge", "--lexical"])
        .env("MYCELIA_CONFIG_HOME", &config_home)
        .env("MYCELIA_DATA_HOME", &data_home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start MCP server");
    let mut stdin = child.stdin.take().expect("MCP stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("MCP stdout"));

    write_json_line(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {
                    "name": "mycelia-test",
                    "version": "0.1.0"
                }
            }
        }),
    );
    let initialized = read_json_line(&mut stdout);
    assert_eq!(initialized["result"]["serverInfo"]["name"], "mycelia");

    write_json_line(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }),
    );
    write_json_line(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        }),
    );
    let tools = read_json_line(&mut stdout);
    let names = tools["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool name"))
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["find", "retrieve"]);

    write_json_line(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "find",
                "arguments": {
                    "query": "precise",
                    "limit": 5
                }
            }
        }),
    );
    let result = read_json_line(&mut stdout);
    let content = result["result"]["content"][0]["text"]
        .as_str()
        .expect("text content");
    assert!(content.contains("\"source_path\":\"notes.txt\""));
    assert!(content.contains("\"chunk_id\""));
    assert!(!content.contains("\"text\""));

    drop(stdin);
    let status = child.wait().expect("wait for MCP server");
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("MCP stderr")
        .read_to_string(&mut stderr)
        .expect("read MCP stderr");
    assert!(status.success(), "MCP server failed:\n{stderr}");
}

#[test]
fn help_flag_exits_zero_on_stdout() {
    for args in [["--help"].as_slice(), ["find", "--help"].as_slice()] {
        let output = run(args);
        assert!(
            output.status.success(),
            "`{args:?}` should exit 0, got {:?}\nstderr:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
        assert!(
            stdout.contains("Usage"),
            "help should print usage:\n{stdout}"
        );
        assert!(
            !stdout.starts_with("error:"),
            "help must not be framed as an error:\n{stdout}"
        );
    }
}

#[test]
fn find_on_missing_database_fails_without_creating_it() {
    let temp = TempDir::new().expect("temp dir");
    let database = temp.path().join("missing.sqlite3");

    let output = run(&[
        "find",
        "hello",
        "--database",
        database.to_str().expect("database path"),
    ]);

    assert!(
        !output.status.success(),
        "find on a missing database should fail, stdout:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        !database.exists(),
        "read command must not create the database file"
    );
}
