use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use tempfile::TempDir;

fn init_git(root: &Path) {
    fs::create_dir_all(root.join(".git")).expect("create .git dir");
}

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
fn connect_without_harness_lists_supported_values() {
    let output = run(["connect"].as_slice());
    assert!(
        !output.status.success(),
        "connect without a harness should fail"
    );
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(
        stderr.contains("Supported harnesses:"),
        "missing supported harness list:\n{stderr}"
    );
    assert!(stderr.contains("codex"), "missing codex harness:\n{stderr}");
    assert!(
        stderr.contains("claude-code"),
        "missing claude-code harness:\n{stderr}"
    );
    assert!(
        stderr.contains("claude-desktop"),
        "missing claude-desktop harness:\n{stderr}"
    );
    assert!(
        stderr.contains("cursor"),
        "missing cursor harness:\n{stderr}"
    );
}

#[test]
fn connect_help_lists_supported_harnesses() {
    let output = run(["connect", "--help"].as_slice());
    assert!(output.status.success(), "connect help should succeed");
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(
        stdout.contains("Supported harnesses:"),
        "missing supported harness list:\n{stdout}"
    );
    assert!(
        stdout.contains("[possible values: codex, claude-code, claude-desktop, cursor, antigravity, opencode, kilo]"),
        "missing possible values:\n{stdout}"
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
fn graph_command_reports_callers_and_callees() {
    let temp = TempDir::new().expect("tempdir");
    let root = temp.path().join("corpus");
    let database = temp.path().join("mycelia.sqlite");
    fs::create_dir_all(&root).expect("create corpus");
    // The callee is defined in one file and called from another: resolution is
    // by name across the corpus.
    write_file(
        &root.join("util.rs"),
        "pub fn helper() -> i32 {\n    42\n}\n",
    );
    write_file(&root.join("app.rs"), "fn run() -> i32 {\n    helper()\n}\n");
    let database = database.to_str().expect("database path");

    run_success(&[
        "index",
        root.to_str().expect("root path"),
        "--database",
        database,
    ]);

    let callers_json = run_success(&[
        "graph",
        "helper",
        "--database",
        database,
        "--direction",
        "callers",
        "--json",
    ]);
    let hits: Value = serde_json::from_str(&callers_json).expect("parse callers json");
    let callers = hits.as_array().expect("callers array");
    assert_eq!(callers.len(), 1, "callers json:\n{callers_json}");
    assert_eq!(callers[0]["symbol"], "run");
    assert_eq!(callers[0]["resolved"], true);

    let callees_text = run_success(&[
        "graph",
        "run",
        "--database",
        database,
        "--direction",
        "callees",
    ]);
    assert!(
        callees_text.contains("symbol: helper"),
        "callees output:\n{callees_text}"
    );
    assert!(
        callees_text.contains("call site line:"),
        "callees output:\n{callees_text}"
    );

    let empty = run_success(&["graph", "nonexistent_symbol", "--database", database]);
    assert!(empty.contains("no callers found"), "empty output:\n{empty}");

    let status = run_success(&["status", "--database", database]);
    assert!(status.contains("graph:"), "status output:\n{status}");
    assert!(status.contains("edges over"), "status output:\n{status}");
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
    fs::write(
        home.join(".codex/config.toml"),
        "model = \"test\"\n\n[mcp_servers.mycelia-forge]\ncommand = \"old\"\nargs = [\"serve\", \"--corpus\", \"forge\"]\n",
    )
    .expect("seed config");
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
    assert_eq!(config.matches("[mcp_servers.mycelia]").count(), 1);
    assert_eq!(config.matches("[mcp_servers.mycelia-forge]").count(), 0);
    assert!(config.contains("command = "));
    assert!(config.contains("args = [\"serve\", \"--corpus\", \"forge\"]"));
}

#[test]
fn connect_new_harnesses_writes_mcp_server_config() {
    let temp = TempDir::new().expect("tempdir");
    let root = temp.path().join("corpus");
    let config_home = temp.path().join("config");
    let data_home = temp.path().join("data");
    let home = temp.path().join("home");
    fs::create_dir_all(&root).expect("create corpus");
    write_file(&root.join("notes.txt"), "hello world");

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
    assert!(setup.status.success());

    for harness in ["antigravity", "opencode", "kilo"] {
        let output = Command::new(bin_path())
            .args(["connect", harness, "--corpus", "forge"])
            .env("MYCELIA_CONFIG_HOME", &config_home)
            .env("MYCELIA_DATA_HOME", &data_home)
            .env("HOME", &home)
            .output()
            .expect("run connect");
        assert!(
            output.status.success(),
            "connect {} failed:\n{}",
            harness,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    assert!(home.join(".gemini/antigravity/mcp_config.json").is_file());
    assert!(home.join(".config/opencode/opencode.json").is_file());
    assert!(home.join(".config/kilo/kilo.json").is_file());

    let agy = fs::read_to_string(home.join(".gemini/antigravity/mcp_config.json")).unwrap();
    assert!(agy.contains("\"mycelia\""));
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
    let other_root = temp.path().join("other-corpus");
    let config_home = temp.path().join("config");
    let data_home = temp.path().join("data");
    fs::create_dir_all(&root).expect("create corpus");
    fs::create_dir_all(&other_root).expect("create other corpus");
    write_file(&root.join("notes.txt"), "a precise sourced result");
    write_file(&other_root.join("other.txt"), "a second corpus result");
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
    run_success_with_homes(
        &[
            "setup",
            other_root.to_str().expect("other root path"),
            "--name",
            "candelabrum",
            "--no-embed",
        ],
        &config_home,
        &data_home,
    );

    let mut child = Command::new(bin_path())
        // Lexical mode keeps the test offline; routing is covered by core tests.
        .args(["serve", "--lexical"])
        .env("MYCELIA_CONFIG_HOME", &config_home)
        .env("MYCELIA_DATA_HOME", &data_home)
        .current_dir(&root)
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
    let instructions = initialized["result"]["instructions"]
        .as_str()
        .expect("server instructions");
    assert!(
        instructions.contains("token-efficient orientation path"),
        "instructions should explain why to use Mycelia first:\n{instructions}"
    );

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
    let listed_tools = tools["result"]["tools"].as_array().expect("tools array");
    let names = listed_tools
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool name"))
        .collect::<Vec<_>>();
    for expected in [
        "find",
        "search_codebase",
        "locate_implementation",
        "retrieve",
        "list_corpora",
    ] {
        assert!(
            names.contains(&expected),
            "missing MCP tool {expected}; got {names:?}"
        );
    }
    let find_description = listed_tools
        .iter()
        .find(|tool| tool["name"] == "find")
        .and_then(|tool| tool["description"].as_str())
        .expect("find description");
    assert!(
        find_description.contains("before grep/read"),
        "find description should position Mycelia as the cheap orientation path:\n{find_description}"
    );

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
    assert!(content.contains("\"chunk_id\":\"forge:"));
    assert!(content.contains("\"corpus\":\"forge\""));
    assert!(!content.contains("\"text\""));
    let headers: Value = serde_json::from_str(content).expect("find headers json");
    let chunk_id = headers[0]["chunk_id"].as_str().expect("chunk id");

    write_json_line(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "retrieve",
                "arguments": {
                    "chunk_id": chunk_id
                }
            }
        }),
    );
    let retrieved = read_json_line(&mut stdout);
    let retrieved_content = retrieved["result"]["content"][0]["text"]
        .as_str()
        .expect("retrieve text content");
    assert!(retrieved_content.contains("\"status\":\"ok\""));
    assert!(retrieved_content.contains("\"corpus\":\"forge\""));

    write_json_line(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "search_codebase",
                "arguments": {
                    "query": "second",
                    "limit": 5,
                    "corpus": "candelabrum"
                }
            }
        }),
    );
    let alias_result = read_json_line(&mut stdout);
    let alias_content = alias_result["result"]["content"][0]["text"]
        .as_str()
        .expect("alias text content");
    assert!(alias_content.contains("\"source_path\":\"other.txt\""));
    assert!(alias_content.contains("\"chunk_id\":\"candelabrum:"));
    assert!(alias_content.contains("\"corpus\":\"candelabrum\""));

    write_json_line(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "tools/call",
            "params": {
                "name": "list_corpora",
                "arguments": {}
            }
        }),
    );
    let corpora = read_json_line(&mut stdout);
    let corpora_content = corpora["result"]["content"][0]["text"]
        .as_str()
        .expect("corpora text content");
    assert!(corpora_content.contains("\"name\":\"forge\""));
    assert!(corpora_content.contains("\"name\":\"candelabrum\""));
    assert!(corpora_content.contains("\"default\":true"));

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

    let stats = run_success_with_homes(
        &["stats", "--recent", "3", "--corpus", "forge"],
        &config_home,
        &data_home,
    );
    assert!(stats.contains("queries answered:  1"));
    assert!(stats.contains("recent activity:"));
    assert!(stats.contains("  find "));
    assert!(stats.contains("  retrieve "));
    assert!(stats.contains("q=\"precise\""));

    let mut needs_child = Command::new(bin_path())
        .args(["serve", "--lexical"])
        .env("MYCELIA_CONFIG_HOME", &config_home)
        .env("MYCELIA_DATA_HOME", &data_home)
        .current_dir(temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start disambiguation MCP server");
    let mut needs_stdin = needs_child.stdin.take().expect("MCP stdin");
    let mut needs_stdout = BufReader::new(needs_child.stdout.take().expect("MCP stdout"));
    write_json_line(
        &mut needs_stdin,
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
    let _ = read_json_line(&mut needs_stdout);
    write_json_line(
        &mut needs_stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }),
    );
    write_json_line(
        &mut needs_stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "find",
                "arguments": {
                    "query": "precise"
                }
            }
        }),
    );
    let needs_result = read_json_line(&mut needs_stdout);
    let needs_content = needs_result["result"]["content"][0]["text"]
        .as_str()
        .expect("needs_corpus text content");
    assert!(needs_content.contains("\"status\":\"needs_corpus\""));
    assert!(needs_content.contains("\"name\":\"forge\""));
    assert!(needs_content.contains("\"name\":\"candelabrum\""));
    drop(needs_stdin);
    let needs_status = needs_child.wait().expect("wait for MCP server");
    let mut needs_stderr = String::new();
    needs_child
        .stderr
        .take()
        .expect("MCP stderr")
        .read_to_string(&mut needs_stderr)
        .expect("read MCP stderr");
    assert!(needs_status.success(), "MCP server failed:\n{needs_stderr}");
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
fn init_creates_project_tree_and_is_idempotent() {
    let temp = TempDir::new().expect("tempdir");
    let root = temp.path();
    init_git(root);
    write_file(&root.join("README.md"), "# hello");

    // First run: decline the guidance include by piping "n".
    let output = Command::new(bin_path())
        .args(["init", "--no-embed", root.to_str().expect("root")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            child.stdin.take(); // drop stdin immediately = EOF => "N" branch
            child.wait_with_output()
        })
        .expect("run init");

    assert!(
        output.status.success(),
        "init failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let mycelia = root.join(".mycelia");
    assert!(mycelia.join("config.toml").is_file(), "config.toml missing");
    assert!(mycelia.join("AGENTS.md").is_file(), "AGENTS.md missing");
    assert!(mycelia.join(".gitignore").is_file(), ".gitignore missing");
    assert!(mycelia.join("db").is_dir(), "db/ missing");
    assert!(mycelia.join("logs").is_dir(), "logs/ missing");
    assert!(mycelia.join("cache").is_dir(), "cache/ missing");
    assert!(
        mycelia.join("db").join("index.sqlite3").is_file(),
        "index.sqlite3 missing after init"
    );

    // Second run must succeed (idempotent) and not duplicate config.toml.
    let second = Command::new(bin_path())
        .args(["init", "--no-embed", root.to_str().expect("root")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            child.stdin.take();
            child.wait_with_output()
        })
        .expect("run init again");

    assert!(
        second.status.success(),
        "second init failed:\n{}",
        String::from_utf8_lossy(&second.stderr)
    );

    // The database path from the first init must still resolve for status.
    let status = Command::new(bin_path())
        .args([
            "status",
            "--database",
            mycelia
                .join("db")
                .join("index.sqlite3")
                .to_str()
                .expect("db"),
        ])
        .output()
        .expect("run status");
    assert!(
        status.status.success(),
        "status after init failed:\n{}",
        String::from_utf8_lossy(&status.stderr)
    );
}

#[test]
fn init_applies_guidance_include_on_confirmation() {
    let temp = TempDir::new().expect("tempdir");
    let root = temp.path();
    init_git(root);
    write_file(&root.join("AGENTS.md"), "# Existing\n\nSome content.\n");

    // Pipe "y\n" to confirm the guidance include.
    let mut child = Command::new(bin_path())
        .args(["init", "--no-embed", root.to_str().expect("root")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn init");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"y\n")
        .expect("write y");
    let output = child.wait_with_output().expect("wait init");

    assert!(
        output.status.success(),
        "init with y failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let agents = fs::read_to_string(root.join("AGENTS.md")).expect("read AGENTS.md");
    assert!(
        agents.contains("<!-- BEGIN mycelia -->"),
        "guidance block not written:\n{agents}"
    );
    assert!(
        agents.contains(".mycelia/AGENTS.md"),
        "guidance block should reference .mycelia/AGENTS.md:\n{agents}"
    );
    assert!(
        agents.contains("Some content."),
        "existing content should be preserved:\n{agents}"
    );
    assert_eq!(
        agents.matches("<!-- BEGIN mycelia -->").count(),
        1,
        "block should appear exactly once:\n{agents}"
    );
}

#[test]
fn init_declining_guidance_leaves_root_file_unchanged() {
    let temp = TempDir::new().expect("tempdir");
    let root = temp.path();
    init_git(root);
    let original = "# My Project\n";
    write_file(&root.join("CLAUDE.md"), original);

    let mut child = Command::new(bin_path())
        .args(["init", "--no-embed", root.to_str().expect("root")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn init");

    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"n\n")
        .expect("write n");
    let output = child.wait_with_output().expect("wait init");

    assert!(
        output.status.success(),
        "init declined failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let claude = fs::read_to_string(root.join("CLAUDE.md")).expect("read CLAUDE.md");
    assert_eq!(
        claude, original,
        "CLAUDE.md must be unchanged when user declines"
    );
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
