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

use anyhow::Context;
use komo_config::ConfigSnapshot;
use komo_config::WikiConfig;
use komo_core::domain::embedding::EmbeddingClient;
use komo_core::domain::wiki::{DIVERSIFY_OVERFETCH, MAX_CHUNKS_PER_FILE, diversify};
use komo_infra::embedding::OllamaEmbedder;
use komo_services::wiki_indexing::index_vault;
use komo_wiki::{WikiBackend, WikiSettings, build_index};

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

pub async fn index(config: &ConfigSnapshot, rebuild: bool) -> anyhow::Result<()> {
    let cfg = wiki_config(config)?;
    let embedder = OllamaEmbedder::new(cfg.embedding.url.clone(), cfg.embedding.model.clone())?;
    embedder.probe().await.with_context(|| {
        format!(
            "embedding backend `{}` at {} is not usable",
            cfg.embedding.model, cfg.embedding.url
        )
    })?;
    let index = build_index(&settings(cfg)?).await?;

    println!(
        "vault    {}\nbackend  {}\nmodel    {}",
        cfg.vault.display(),
        cfg.backend,
        cfg.embedding.model
    );
    if rebuild {
        println!("(rebuilding from scratch)");
    }

    let outcome = index_vault(
        &*index,
        &embedder,
        &cfg.vault,
        &cfg.embedding.model,
        rebuild,
        |progress| {
            use std::io::Write;
            print!("\r  embedded {} chunks", progress.chunks_written);
            let _ = std::io::stdout().flush();
        },
    )
    .await?;

    println!(
        "\rfiles    {} ({} changed, {} removed, {} unchanged)    ",
        outcome.files_seen,
        outcome.files_changed,
        outcome.files_removed,
        outcome.files_seen - outcome.files_changed,
    );
    for skipped in &outcome.skipped {
        eprintln!("  skip {skipped}");
    }
    if outcome.chunks_written == 0 && outcome.files_removed == 0 {
        println!(
            "nothing to do — index is current ({} chunks)",
            outcome.chunks_total
        );
    } else {
        println!("index now holds {} chunks", outcome.chunks_total);
    }
    Ok(())
}

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
