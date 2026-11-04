//! Validate a curated manifest, optionally enriching a private replay archive.
use anyhow::{ensure, Context, Result};
use pitwall_core::SessionArchive;
use pitwall_ingest::metadata::MetadataManifest;
use std::{
    fs::File,
    io::{BufWriter, Write},
};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 1 || args.len() == 3,
        "Usage: import_metadata MANIFEST [INPUT_ARCHIVE OUTPUT_ARCHIVE]"
    );
    let manifest = MetadataManifest::from_path(&args[0])?;
    if args.len() == 1 {
        println!("Metadata manifest validated");
        return Ok(());
    }
    let mut archive: SessionArchive =
        serde_json::from_reader(File::open(&args[1])?).context("Read private session archive")?;
    ensure!(
        manifest.apply(&mut archive.session, &mut archive.circuit)?,
        "Manifest does not cover archive session {}",
        archive.session.key
    );
    let output = File::create_new(&args[2])
        .context("Create new output archive (existing files are preserved)")?;
    let mut output = BufWriter::new(output);
    serde_json::to_writer(&mut output, &archive)?;
    output.flush()?;
    println!("Applied metadata to session {}", archive.session.key);
    Ok(())
}
