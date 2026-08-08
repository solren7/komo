//! `komo wiki index|status` — build and inspect the note-vault index.
//!
//! Indexing is **incremental by mtime**: a note whose file has not changed since
//! it was indexed is never read, chunked, or embedded again. That is what keeps
//! a re-run cheap enough to put on a schedule — embedding is the expensive part
//! (a round trip per batch), and skipping unchanged files skips all of it.
//!
//! Unlike `komo memory`, this does not route through `operator_control`: the
//! index is its own store, not one of the Turso dbs the gateway holds an
//! exclusive lock on. That stays true only while the gateway does not hold the
//! index open itself — once `wiki_search` is wired into the running gateway,
//! this must move behind the operator channel like the memory commands did.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::Context;
use komo_config::ConfigSnapshot;
use komo_config::WikiConfig;
use komo_core::domain::embedding::EmbeddingClient;
use komo_core::domain::wiki::{
    DIVERSIFY_OVERFETCH, MAX_CHUNKS_PER_FILE, WikiChunk, WikiIndex, diversify,
};
use komo_infra::embedding::OllamaEmbedder;
use komo_services::wiki_chunking::{ChunkSpec, chunk_markdown};
use komo_wiki::{WikiBackend, WikiSettings, build_index};

/// How many chunks are embedded per request. Ollama handles a batch far faster
/// than the same chunks one at a time, but an unbounded batch risks the
/// backend's own request limits on a large note.
const EMBED_BATCH: usize = 32;

fn settings(cfg: &WikiConfig) -> anyhow::Result<WikiSettings> {
    Ok(WikiSettings {
        backend: WikiBackend::parse(&cfg.backend)?,
        data_dir: cfg.data_dir.clone(),
        url: cfg.url.clone(),
        collection: cfg.collection.clone(),
        // Credentials never come from config.toml; this mirrors how the
        // channels read their tokens.
        api_key: std::env::var("QDRANT_API_KEY").ok(),
    })
}

fn wiki_config(config: &ConfigSnapshot) -> anyhow::Result<&WikiConfig> {
    config.runtime.wiki.as_ref().context(
        "no [wiki] configured. Add a vault path to ~/.komo/config.toml:\n\n\
         [wiki]\n\
         vault = \"~/02-note/01-note\"\n",
    )
}

/// Every `.md` under `root`, skipping dot-directories (`.obsidian`, `.trash`).
fn walk(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            // A vanished or unreadable subdirectory must not abort a whole
            // index run over one bad directory.
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "md") {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

fn mtime_of(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub async fn index(config: &ConfigSnapshot, rebuild: bool) -> anyhow::Result<()> {
    let cfg = wiki_config(config)?;
    if !cfg.vault.is_dir() {
        anyhow::bail!("vault not found: {}", cfg.vault.display());
    }
    let embedder = OllamaEmbedder::new(cfg.embedding.url.clone(), cfg.embedding.model.clone())?;
    embedder.probe().await.with_context(|| {
        format!(
            "embedding backend `{}` at {} is not usable",
            cfg.embedding.model, cfg.embedding.url
        )
    })?;
    let index = build_index(&settings(cfg)?).await?;

    let files = walk(&cfg.vault)?;
    let indexed = if rebuild {
        // Drop the store, not just its contents: vector width is fixed when the
        // index is created, so changing embedding model is only possible this
        // way. Everything here is rebuilt from the `.md` files below.
        index.reset().await?;
        println!("(rebuilding from scratch)");
        HashMap::new()
    } else {
        index.indexed().await?
    };

    // Vault-relative paths are what the index stores, so the whole diff is done
    // in that space — an absolute path would break the moment the vault moves.
    let mut on_disk: HashMap<String, (PathBuf, i64)> = HashMap::new();
    for path in &files {
        let rel = path
            .strip_prefix(&cfg.vault)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();
        on_disk.insert(rel, (path.clone(), mtime_of(path)));
    }

    let changed: Vec<&String> = on_disk
        .iter()
        .filter(|(rel, (_, mtime))| indexed.get(*rel).is_none_or(|had| had.mtime != *mtime))
        .map(|(rel, _)| rel)
        .collect();
    let live: HashSet<&str> = on_disk.keys().map(String::as_str).collect();
    let removed: Vec<String> = indexed
        .keys()
        .filter(|rel| !live.contains(rel.as_str()))
        .cloned()
        .collect();

    println!(
        "vault    {}\nbackend  {}\nmodel    {}\nfiles    {} ({} changed, {} removed, {} unchanged)",
        cfg.vault.display(),
        cfg.backend,
        cfg.embedding.model,
        files.len(),
        changed.len(),
        removed.len(),
        files.len() - changed.len(),
    );

    if !removed.is_empty() {
        index.delete_paths(&removed).await?;
    }
    if changed.is_empty() && removed.is_empty() {
        println!(
            "\nnothing to do — index is current ({} chunks)",
            index.count().await?
        );
        return Ok(());
    }
    // A changed file's old chunks must go before the new ones land: a note that
    // got shorter would otherwise keep orphaned tail chunks forever.
    let changed_owned: Vec<String> = changed.iter().map(|s| (*s).to_string()).collect();
    if !changed_owned.is_empty() && !rebuild {
        index.delete_paths(&changed_owned).await?;
    }

    let spec = ChunkSpec::default();
    let mut pending: Vec<WikiChunk> = Vec::new();
    let mut total_chunks = 0usize;

    for rel in &changed_owned {
        let (path, mtime) = &on_disk[rel];
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("  skip {rel}: {e}");
                continue;
            }
        };
        let title = path.file_stem().unwrap_or_default().to_string_lossy();
        for raw in chunk_markdown(&title, &content, &spec) {
            pending.push(WikiChunk {
                id: WikiChunk::make_id(rel, raw.ordinal),
                path: rel.clone(),
                heading_path: raw.heading_path,
                ordinal: raw.ordinal,
                text: raw.text,
                mtime: *mtime,
                embedding: Vec::new(),
                embedding_model: cfg.embedding.model.clone(),
            });
        }
        while pending.len() >= EMBED_BATCH {
            let batch: Vec<WikiChunk> = pending.drain(..EMBED_BATCH).collect();
            total_chunks += flush(&*index, &embedder, batch).await?;
            print!("\r  embedded {total_chunks} chunks");
            use std::io::Write;
            let _ = std::io::stdout().flush();
        }
    }
    if !pending.is_empty() {
        total_chunks += flush(&*index, &embedder, pending).await?;
    }

    println!("\r  embedded {total_chunks} chunks    ");
    println!("\nindex now holds {} chunks", index.count().await?);
    Ok(())
}

/// Embed one batch and write it.
async fn flush(
    index: &dyn WikiIndex,
    embedder: &OllamaEmbedder,
    mut batch: Vec<WikiChunk>,
) -> anyhow::Result<usize> {
    let texts: Vec<String> = batch
        .iter()
        // The heading trail is embedded with the body: a chunk under
        // "checkout policy > 状态机" should match a query naming either, and the
        // body alone often names neither.
        .map(|c| format!("{}\n{}", c.heading_path, c.text))
        .collect();
    let vectors = embedder.embed(&texts).await?;
    if vectors.len() != batch.len() {
        anyhow::bail!(
            "embedding backend returned {} vectors for {} chunks",
            vectors.len(),
            batch.len()
        );
    }
    for (chunk, vector) in batch.iter_mut().zip(vectors) {
        chunk.embedding = vector;
    }
    let n = batch.len();
    index.upsert(&batch).await?;
    Ok(n)
}

/// Run one query against the index, exactly as the `wiki_search` tool does.
///
/// The operator-facing twin of that tool: same embedding, same index, same
/// floor — so "does the agent find this note?" can be answered without a turn.
pub async fn search(config: &ConfigSnapshot, query: &str, limit: usize) -> anyhow::Result<()> {
    let cfg = wiki_config(config)?;
    let embedder = OllamaEmbedder::new(cfg.embedding.url.clone(), cfg.embedding.model.clone())?;
    let index = build_index(&settings(cfg)?).await?;

    let vectors = embedder.embed(&[query.to_string()]).await?;
    let vector = vectors
        .into_iter()
        .next()
        .filter(|v| !v.is_empty())
        .context("embedding backend returned no vector")?;

    // Same over-fetch-then-cap as the tool, so this keeps predicting what a turn
    // actually gets back.
    let candidates = index
        .search(&vector, query, limit * DIVERSIFY_OVERFETCH, SEARCH_FLOOR)
        .await?;
    let hits = diversify(candidates, limit, MAX_CHUNKS_PER_FILE);
    if hits.is_empty() {
        println!("no matches above {SEARCH_FLOOR:.2}");
        return Ok(());
    }
    for hit in hits {
        let preview: String = hit.chunk.text.chars().take(160).collect();
        println!(
            "\n── {} ({:.3})\n   {}\n   {}",
            hit.chunk.path,
            hit.score,
            hit.chunk.heading_path,
            preview.replace('\n', " ")
        );
    }
    Ok(())
}

/// Kept in step with `wiki_search`'s own floor — this command exists to predict
/// what that tool will return, so a different threshold would make it lie.
const SEARCH_FLOOR: f32 = 0.45;

pub async fn status(config: &ConfigSnapshot) -> anyhow::Result<()> {
    let cfg = wiki_config(config)?;
    let index = build_index(&settings(cfg)?).await?;
    let indexed = index.indexed().await?;
    let chunks = index.count().await?;

    println!("vault      {}", cfg.vault.display());
    println!("backend    {}", cfg.backend);
    if WikiBackend::parse(&cfg.backend)? == WikiBackend::Server {
        println!("url        {}", cfg.url);
    } else {
        println!(
            "data       {}",
            cfg.data_dir.join(&cfg.collection).display()
        );
    }
    println!("collection {}", cfg.collection);
    println!("model      {} @ {}", cfg.embedding.model, cfg.embedding.url);
    println!("indexed    {} files, {chunks} chunks", indexed.len());
    match index.vector_spec().await? {
        Some((dim, model)) if !model.is_empty() => {
            println!("vectors    {dim}-dim, written by `{model}`");
            if model != cfg.embedding.model {
                println!(
                    "\n! index was built with `{model}` but config says `{}`.\n\
                       Vectors from different models are not comparable — run \
                     `komo wiki index --rebuild`.",
                    cfg.embedding.model
                );
            }
        }
        Some((dim, _)) => println!("vectors    {dim}-dim"),
        None => println!("vectors    (empty — run `komo wiki index`)"),
    }
    Ok(())
}
