use csv::Writer;
use ferrous_frog_storage::CrawlRecord;
use std::io::Write;

pub fn records_to_csv<W: Write>(records: &[CrawlRecord], writer: W) -> csv::Result<()> {
    let mut writer = Writer::from_writer(writer);
    writer.write_record([
        "id",
        "url",
        "final_url",
        "classification",
        "status_code",
        "status_text",
        "content_type",
        "indexability",
        "indexability_status",
        "response_time_ms",
        "size_bytes",
        "depth",
        "redirect_target",
        "title",
        "title_len",
        "meta_description",
        "meta_description_len",
        "h1",
        "h1_len",
        "canonical",
        "inlink_count",
        "outlink_count",
        "internal_outlink_count",
        "external_outlink_count",
        "error",
    ])?;

    for record in records {
        writer.write_record([
            record.id.to_string(),
            record.url.clone(),
            record.final_url.clone(),
            format!("{:?}", record.classification),
            record
                .status_code
                .map(|code| code.to_string())
                .unwrap_or_default(),
            record.status_text.clone(),
            record.content_type.clone().unwrap_or_default(),
            record.indexability.clone(),
            record.indexability_status.clone(),
            record.response_time_ms.to_string(),
            record.size_bytes.to_string(),
            record.depth.to_string(),
            record.redirect_target.clone().unwrap_or_default(),
            record.title.clone().unwrap_or_default(),
            record.title_len.to_string(),
            record.meta_description.clone().unwrap_or_default(),
            record.meta_description_len.to_string(),
            record.h1.clone().unwrap_or_default(),
            record.h1_len.to_string(),
            record.canonical.clone().unwrap_or_default(),
            record.inlink_count.to_string(),
            record.outlink_count.to_string(),
            record.internal_outlink_count.to_string(),
            record.external_outlink_count.to_string(),
            record.error.clone().unwrap_or_default(),
        ])?;
    }

    writer.flush()?;
    Ok(())
}

pub fn records_to_csv_string(records: &[CrawlRecord]) -> csv::Result<String> {
    let mut bytes = Vec::new();
    records_to_csv(records, &mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrous_frog_storage::CrawlRecord;

    #[test]
    fn writes_csv_headers_and_rows() {
        let mut record = CrawlRecord::pending("https://example.com/".to_string(), 0);
        record.status_code = Some(200);
        record.title = Some("Home".to_string());

        let csv = records_to_csv_string(&[record]).unwrap();

        assert!(csv.contains("final_url"));
        assert!(csv.contains("https://example.com/"));
    }
}
