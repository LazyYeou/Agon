use super::*;

impl Store {
    pub async fn export_parquet(&self, key: u32) -> Result<PathBuf> {
        let archive = match self.load_live_archive(key).await? {
            Some(archive) => archive,
            None => self.load(key).await?.context("Session archive not found")?,
        };
        let bytes = tokio::task::spawn_blocking(move || encode(&archive)).await??;
        let name = format!("{key}.parquet");
        self.atomic(&name, &bytes).await?;
        Ok(self.root.join(name))
    }
}

fn encode(archive: &SessionArchive) -> Result<Vec<u8>> {
    use parquet::{
        data_type::{ByteArray, ByteArrayType, Int64Type},
        file::writer::SerializedFileWriter,
        schema::parser::parse_message_type,
    };
    let schema = std::sync::Arc::new(parse_message_type(
        "message events { REQUIRED INT64 time_ms; REQUIRED BINARY event_json (UTF8); }",
    )?);
    let mut writer = SerializedFileWriter::new(Vec::new(), schema, Default::default())?;
    // Keep one bounded row group in memory while retaining the complete event document.
    for events in archive.events.chunks(4096) {
        let mut group = writer.next_row_group()?;
        let times: Vec<_> = events.iter().map(|e| e.time_ms).collect();
        let documents: Vec<ByteArray> = events
            .iter()
            .map(serde_json::to_vec)
            .collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .map(ByteArray::from)
            .collect();
        let mut column = group.next_column()?.context("Missing timestamp column")?;
        column
            .typed::<Int64Type>()
            .write_batch(&times, None, None)?;
        column.close()?;
        let mut column = group.next_column()?.context("Missing event column")?;
        column
            .typed::<ByteArrayType>()
            .write_batch(&documents, None, None)?;
        column.close()?;
        group.close()?;
    }
    Ok(writer.into_inner()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use parquet::{
        file::reader::{FileReader, SerializedFileReader},
        record::RowAccessor,
    };
    #[test]
    fn parquet_preserves_event_time_and_lossless_event_document() {
        let mut archive = pitwall_state::demo_archive();
        archive.events.truncate(2);
        let bytes = encode(&archive).unwrap();
        let path = std::env::temp_dir().join(format!("agon-export-{}.parquet", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        let reader = SerializedFileReader::new(std::fs::File::open(&path).unwrap()).unwrap();
        let rows = reader
            .get_row_iter(None)
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get_long(0).unwrap(), archive.events[0].time_ms);
        let restored: RaceEvent = serde_json::from_str(rows[1].get_string(1).unwrap()).unwrap();
        assert_eq!(restored, archive.events[1]);
        drop(reader);
        std::fs::remove_file(path).unwrap();
    }
}
